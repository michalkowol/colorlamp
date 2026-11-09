use crate::protocol::{COMPANY_ID, UUID_SCAN, UUID_SERVICE, UUID_WRITE};
use anyhow::{Result, anyhow};
use btleplug::api::{Central, CentralEvent, CharPropFlags, Characteristic, Manager as _, Peripheral as _, ScanFilter, WriteType};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::StreamExt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const DISCOVER_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug)]
pub struct Advertisement {
    pub id: String,
    pub name: Option<String>,
    pub rssi: Option<i16>,
    // [group, type] from manufacturer data with company id 22872
    pub manufacturer: Option<Vec<u8>>,
    pub is_lamp: bool
}

#[derive(Clone, Debug)]
pub enum BleEvent {
    Advertisement(Advertisement),
    Disconnected(String)
}

struct Link {
    peripheral: Peripheral,
    char: Characteristic
}

struct Job {
    bytes: Vec<u8>,
    confirmed: bool,
    done: oneshot::Sender<Result<()>>
}

// Each lamp has one queue, so GATT writes never overlap and keep the order they were sent in.
#[derive(Clone)]
pub struct Ble {
    adapter: Option<Adapter>,
    links: Arc<Mutex<HashMap<String, Link>>>,
    queues: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<Job>>>>,
    scanning: Arc<Mutex<bool>>,
    // BlueZ and CoreBluetooth handle one connection attempt at a time reliably; parallel attempts
    // to several lamps make some of them fail.
    connecting: Arc<tokio::sync::Mutex<()>>
}

impl Ble {
    pub async fn new() -> (Ble, Option<String>) {
        let mut error = None;
        let adapter = match Manager::new().await {
            Ok(manager) => match manager.adapters().await {
                Ok(list) => {
                    let first = list.into_iter().next();
                    if first.is_none() {
                        error = Some("No Bluetooth adapter found.".to_string());
                    }
                    first
                }
                Err(e) => {
                    error = Some(format!("Bluetooth is not available: {e}"));
                    None
                }
            },
            Err(e) => {
                error = Some(format!("Bluetooth is not available: {e}"));
                None
            }
        };
        let ble = Ble {
            adapter,
            links: Arc::new(Mutex::new(HashMap::new())),
            queues: Arc::new(Mutex::new(HashMap::new())),
            scanning: Arc::new(Mutex::new(false)),
            connecting: Arc::new(tokio::sync::Mutex::new(()))
        };
        (ble, error)
    }

    pub fn available(&self) -> bool {
        self.adapter.is_some()
    }

    fn adapter(&self) -> Result<&Adapter> {
        self.adapter.as_ref().ok_or_else(|| anyhow!("no Bluetooth adapter"))
    }

    pub async fn events(&self) -> Result<mpsc::UnboundedReceiver<BleEvent>> {
        let adapter = self.adapter()?.clone();
        let mut stream = adapter.events().await?;
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Some(event) = stream.next().await {
                let id = match &event {
                    CentralEvent::DeviceDiscovered(id)
                    | CentralEvent::DeviceUpdated(id)
                    | CentralEvent::ManufacturerDataAdvertisement { id, .. }
                    | CentralEvent::ServicesAdvertisement { id, .. }
                    | CentralEvent::RssiUpdate { id, .. } => id.clone(),
                    CentralEvent::DeviceDisconnected(id) => {
                        if tx.send(BleEvent::Disconnected(id.to_string())).is_err() {
                            return;
                        }
                        continue;
                    }
                    _ => continue
                };
                let Ok(p) = adapter.peripheral(&id).await else { continue };
                let Ok(Some(props)) = p.properties().await else { continue };
                let manufacturer = props.manufacturer_data.get(&COMPANY_ID).cloned();
                let adv = Advertisement {
                    id: id.to_string(),
                    name: props.local_name.or(props.advertisement_name),
                    rssi: props.rssi,
                    is_lamp: props.services.contains(&UUID_SCAN) || manufacturer.is_some(),
                    manufacturer
                };
                if tx.send(BleEvent::Advertisement(adv)).is_err() {
                    return;
                }
            }
        });
        Ok(rx)
    }

    // Scans without a filter: lamps match either the advertised service 0xAF30 or the manufacturer
    // data company id (the browser chooser used both filters).
    pub async fn set_scanning(&self, on: bool) -> Result<()> {
        let adapter = self.adapter()?;
        {
            let mut scanning = self.scanning.lock().unwrap();
            if *scanning == on {
                return Ok(());
            }
            *scanning = on;
        }
        let result = if on { adapter.start_scan(ScanFilter::default()).await } else { adapter.stop_scan().await };
        if let Err(e) = result {
            *self.scanning.lock().unwrap() = !on;
            return Err(e.into());
        }
        Ok(())
    }

    async fn peripheral(&self, id: &str) -> Result<Peripheral> {
        let adapter = self.adapter()?;
        adapter
            .peripherals()
            .await?
            .into_iter()
            .find(|p| p.id().to_string() == id)
            .ok_or_else(|| anyhow!("lamp not in range"))
    }

    pub async fn advertisement(&self, id: &str) -> Option<Advertisement> {
        let p = self.peripheral(id).await.ok()?;
        let props = p.properties().await.ok()??;
        let manufacturer = props.manufacturer_data.get(&COMPANY_ID).cloned();
        Some(Advertisement {
            id: id.to_string(),
            name: props.local_name.or(props.advertisement_name),
            rssi: props.rssi,
            is_lamp: props.services.contains(&UUID_SCAN) || manufacturer.is_some(),
            manufacturer
        })
    }

    pub async fn connect(&self, id: &str) -> Result<()> {
        let _turn = self.connecting.lock().await;
        let p = self.peripheral(id).await?;
        if !p.is_connected().await.unwrap_or(false) {
            p.connect_with_timeout(CONNECT_TIMEOUT).await?;
        }
        p.discover_services_with_timeout(DISCOVER_TIMEOUT).await?;
        let char = p
            .characteristics()
            .into_iter()
            .find(|c| c.uuid == UUID_WRITE && c.service_uuid == UUID_SERVICE)
            .ok_or_else(|| anyhow!("write characteristic 0xAE01 not found"))?;
        self.links.lock().unwrap().insert(id.to_string(), Link { peripheral: p, char });
        Ok(())
    }

    pub fn drop_link(&self, id: &str) {
        self.links.lock().unwrap().remove(id);
    }

    pub async fn disconnect(&self, id: &str) {
        let link = self.links.lock().unwrap().remove(id);
        let p = match link {
            Some(l) => Some(l.peripheral),
            None => self.peripheral(id).await.ok()
        };
        if let Some(p) = p
            && p.is_connected().await.unwrap_or(false)
        {
            let _ = p.disconnect().await;
        }
    }

    // Enqueues the write now; the receiver resolves once it was written (or skipped without a link).
    // confirmed: wait for the BLE-level acknowledgement when the characteristic allows it.
    pub fn enqueue(&self, id: &str, bytes: Vec<u8>, confirmed: bool) -> oneshot::Receiver<Result<()>> {
        let (done, rx) = oneshot::channel();
        let mut queues = self.queues.lock().unwrap();
        let sender = queues.entry(id.to_string()).or_insert_with(|| self.spawn_worker(id.to_string()));
        if let Err(e) = sender.send(Job { bytes, confirmed, done }) {
            let _ = e.0.done.send(Ok(()));
        }
        rx
    }

    // Resolves once every write queued before it has been sent.
    pub async fn barrier(&self, id: &str) {
        let _ = self.enqueue(id, Vec::new(), false).await;
    }

    fn spawn_worker(&self, id: String) -> mpsc::UnboundedSender<Job> {
        let (tx, mut rx) = mpsc::unbounded_channel::<Job>();
        let links = self.links.clone();
        tokio::spawn(async move {
            while let Some(job) = rx.recv().await {
                let link = links.lock().unwrap().get(&id).map(|l| (l.peripheral.clone(), l.char.clone()));
                let result = match link {
                    _ if job.bytes.is_empty() => Ok(()),
                    None => Ok(()),
                    Some((p, c)) => {
                        let props = c.properties;
                        let write_type = if job.confirmed && props.contains(CharPropFlags::WRITE) {
                            WriteType::WithResponse
                        } else if props.contains(CharPropFlags::WRITE_WITHOUT_RESPONSE) {
                            WriteType::WithoutResponse
                        } else {
                            WriteType::WithResponse
                        };
                        p.write(&c, &job.bytes, write_type).await.map_err(|e| anyhow!(e))
                    }
                };
                let _ = job.done.send(result);
            }
        });
        tx
    }
}
