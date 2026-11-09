// Static segments. No per-LED command exists. LEDs past the declared IC length (0F) keep their last
// colour, so we paint the top colour at full length, then shrink the length and paint lower segments.
use crate::protocol::{CLASSIC_NAMES, CLASSIC_RGB, MAX_LEDS, MIN_LEDS, PALETTE_ORDER, ic_length_packet, js_round, rgb_packet};
use crate::store::Segment;

pub const MAX_SEGMENTS: usize = 6;

#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub index: usize,
    pub label: String,
    pub packets: Vec<Vec<u8>>
}

// Split the strip into equal parts; the first end is clamped to the controller minimum of 16 LEDs.
pub fn split_evenly(segments: &mut [Segment], full: i64) {
    if full < MIN_LEDS {
        return;
    }
    let k = segments.len();
    let mut prev = 0;
    for (i, seg) in segments.iter_mut().enumerate() {
        if i == k - 1 {
            seg.end = None;
            continue;
        }
        let end = MIN_LEDS.max(prev + 1).max(js_round(full as f64 * (i + 1) as f64 / k as f64) as i64);
        seg.end = Some(end);
        prev = end;
    }
}

// New segment goes to the bottom of the lamp by splitting the current bottom segment in half;
// every boundary already set stays where it is.
pub fn add_segment(segments: &mut Vec<Segment>) -> Result<(), String> {
    if segments.len() >= MAX_SEGMENTS {
        return Ok(());
    }
    let bottom = &segments[0];
    let bottom_end = bottom.end.unwrap_or(0);
    let end = MIN_LEDS.max(js_round(bottom_end as f64 / 2.0) as i64);
    if end >= bottom_end {
        return Err("The bottom segment is too short to split (at least 16 LEDs at the bottom).".to_string());
    }
    let pos = PALETTE_ORDER.iter().position(|c| *c == bottom.color).map(|p| p as i64).unwrap_or(-1);
    let color = PALETTE_ORDER[((pos + 3) as usize) % PALETTE_ORDER.len()];
    segments.insert(0, Segment { color, end: Some(end) });
    Ok(())
}

// Removing a segment lets its neighbour take the space; other boundaries stay unchanged.
pub fn remove_segment(segments: &mut Vec<Segment>, i: usize) {
    if segments.len() <= 2 || i >= segments.len() {
        return;
    }
    segments.remove(i);
    if let Some(last) = segments.last_mut() {
        last.end = None;
    }
}

fn ends(segments: &[Segment], full: i64) -> Option<Vec<i64>> {
    let k = segments.len();
    segments.iter().enumerate().map(|(j, seg)| if j == k - 1 { Some(full) } else { seg.end }).collect()
}

// Segment sizes rounded to whole percent with the largest remainder method, so they add up
// to exactly 100%; "up to X%" is the running sum from the bottom.
pub fn segment_percent(segments: &[Segment], full: i64, i: usize) -> String {
    if full <= 0 {
        return String::new();
    }
    let Some(ends) = ends(segments, full) else {
        return String::new();
    };
    let sizes: Vec<i64> = ends.iter().enumerate().map(|(j, e)| 0.max(e - if j == 0 { 0 } else { ends[j - 1] })).collect();
    let total: i64 = sizes.iter().sum();
    if total <= 0 {
        return String::new();
    }
    let exact: Vec<f64> = sizes.iter().map(|v| *v as f64 / total as f64 * 100.0).collect();
    let mut pct: Vec<i64> = exact.iter().map(|v| v.floor() as i64).collect();
    let mut rest = 100 - pct.iter().sum::<i64>();
    let mut order: Vec<(f64, usize)> = exact.iter().enumerate().map(|(j, v)| (v - v.floor(), j)).collect();
    order.sort_by(|p, q| q.0.partial_cmp(&p.0).unwrap_or(std::cmp::Ordering::Equal));
    for (_, j) in order {
        if rest > 0 {
            pct[j] += 1;
            rest -= 1;
        }
    }
    let up_to: i64 = pct[..=i].iter().sum();
    format!("up to {up_to}% · segment {}%", pct[i])
}

pub fn segments_error(segments: &[Segment], full: i64) -> Option<String> {
    if !(MIN_LEDS..=MAX_LEDS).contains(&full) {
        return Some("Enter the full LED count of the strip (16–2048).".to_string());
    }
    let n = segments.len();
    let mut prev = 0;
    for (i, seg) in segments.iter().enumerate().take(n - 1) {
        let label = n - i;
        let Some(end) = seg.end.filter(|e| *e >= MIN_LEDS) else {
            return Some(format!("Segment {label}: the boundary must be at LED 16 or higher."));
        };
        if end <= prev {
            return Some(format!("Segment {label}: the boundary must be above the segment below it."));
        }
        if end >= full {
            return Some(format!("Segment {label}: the boundary must be below the full LED count ({full})."));
        }
        prev = end;
    }
    None
}

// Whole strip first (top colour), then shrink and paint lower segments.
// The reverse order (grow from the bottom) does not work on the lamp.
pub fn segment_steps(segments: &[Segment], full: i64) -> Vec<Step> {
    let k = segments.len();
    let mut steps: Vec<Step> = segments
        .iter()
        .enumerate()
        .map(|(i, seg)| {
            let end = if i == k - 1 { full.max(0) } else { seg.end.unwrap_or(0) };
            let color = seg.color.min(CLASSIC_RGB.len() - 1);
            Step {
                index: i,
                label: format!("LEDs 1–{end}: {}", CLASSIC_NAMES[color]),
                packets: vec![ic_length_packet(end as u16), rgb_packet(CLASSIC_RGB[color])]
            }
        })
        .collect();
    steps.reverse();
    steps
}

pub fn segments_name(segments: &[Segment]) -> String {
    let names: Vec<&str> = segments.iter().rev().map(|s| CLASSIC_NAMES[s.color.min(15)]).collect();
    format!("Segments: {}", names.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(color: usize, end: Option<i64>) -> Segment {
        Segment { color, end }
    }

    #[test]
    fn steps_paint_top_first() {
        let segs = vec![seg(0, Some(42)), seg(2, None)];
        let steps = segment_steps(&segs, 84);
        assert_eq!(steps[0].index, 1);
        assert_eq!(steps[0].packets, vec![vec![0x0f, 84, 0], vec![0x03, 0, 0, 255, 0, 0, 0]]);
        assert_eq!(steps[1].packets, vec![vec![0x0f, 42, 0], vec![0x03, 255, 0, 0, 0, 0, 0]]);
        assert_eq!(steps[1].label, "LEDs 1–42: red");
    }

    #[test]
    fn split_and_add() {
        let mut segs = vec![seg(0, Some(10)), seg(4, Some(20)), seg(2, None)];
        split_evenly(&mut segs, 30);
        assert_eq!(segs.iter().map(|s| s.end).collect::<Vec<_>>(), vec![Some(16), Some(20), None]);

        let mut segs = vec![seg(0, Some(42)), seg(2, None)];
        add_segment(&mut segs).unwrap();
        assert_eq!(segs[0], seg(12, Some(21)));
        assert!(add_segment(&mut vec![seg(0, Some(16)), seg(2, None)]).is_err());
    }

    #[test]
    fn percent_adds_up() {
        let segs = vec![seg(0, Some(28)), seg(1, Some(56)), seg(2, None)];
        assert_eq!(segment_percent(&segs, 84, 0), "up to 34% · segment 34%");
        assert_eq!(segment_percent(&segs, 84, 2), "up to 100% · segment 33%");
    }

    #[test]
    fn errors() {
        assert!(segments_error(&[seg(0, Some(42)), seg(2, None)], 84).is_none());
        assert_eq!(segments_error(&[seg(0, Some(10)), seg(2, None)], 84).unwrap(), "Segment 2: the boundary must be at LED 16 or higher.");
        assert_eq!(
            segments_error(&[seg(0, Some(84)), seg(2, None)], 84).unwrap(),
            "Segment 2: the boundary must be below the full LED count (84)."
        );
        assert_eq!(
            segments_error(&[seg(0, Some(40)), seg(1, Some(30)), seg(2, None)], 84).unwrap(),
            "Segment 2: the boundary must be above the segment below it."
        );
        assert!(segments_error(&[seg(0, Some(42)), seg(2, None)], 8).is_some());
    }
}
