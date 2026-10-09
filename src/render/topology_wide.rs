//! Fold horizontal stage flow into connected bands within a 120-column target.
use super::{Event, Product, Step};
use crate::render::topology::write_row;

const WIDTH: usize = 120;

struct Placement {
    band: usize,
    start: usize,
    operation: usize,
    box_width: usize,
    end: usize,
}

struct Label<'a> {
    x: usize,
    y: usize,
    name: &'a str,
    product: bool,
}

struct Canvas<'a> {
    edges: Vec<u8>,
    arrows: Vec<(usize, usize, char)>,
    labels: Vec<Label<'a>>,
    boxes: Vec<(usize, usize, usize)>,
    margin: usize,
    inner: usize,
    pitch: usize,
}

/// Layout planning is linear in inputs/outputs; drawing follows output size.
/// Return false before drawing if a label or live frontier cannot fit.
pub(super) fn render(
    steps: &[&Step<'_>],
    products: &[Product<'_>],
    events: &[Event],
    lanes: usize,
    text: &mut String,
) -> bool {
    let margin = lanes * 2 + 2;
    let Some(inner) = WIDTH.checked_sub(margin * 2) else {
        return false;
    };
    let mut placements = Vec::with_capacity(events.len());
    let mut band = 0;
    let mut cursor = 0;
    for (step, event) in steps.iter().zip(events) {
        let sources = event
            .sources
            .iter()
            .map(|&id| products[id].name.chars().count() + 2)
            .max()
            .unwrap_or(0);
        let outputs = event
            .outputs
            .iter()
            .map(|&id| products[id].name.chars().count() + 2)
            .max()
            .unwrap_or(0);
        let box_width = (step.name.chars().count() + 4).max(event.inputs.len() * 2 + 2);
        let size = sources + if sources == 0 { 0 } else { 3 } + box_width + outputs + 6;
        if size >= inner {
            return false;
        }
        if cursor + size >= inner {
            band += 1;
            cursor = 0;
        }
        let operation = cursor + sources + if sources == 0 { 0 } else { 3 };
        placements.push(Placement {
            band,
            start: cursor,
            operation,
            box_width,
            end: cursor + size,
        });
        cursor += size;
    }
    let pitch = lanes * 2 + 8;
    let mut canvas = Canvas {
        edges: vec![0; (band + 1) * pitch * WIDTH],
        arrows: Vec::new(),
        labels: Vec::new(),
        boxes: Vec::new(),
        margin,
        inner,
        pitch,
    };
    let mut active = vec![false; lanes];
    let mut remaining = vec![0; products.len()];
    for event in events {
        for &id in &event.inputs {
            remaining[id] += 1;
        }
    }
    let mut previous_band = 0;
    let mut previous_end = 0;
    for ((step, event), place) in steps.iter().zip(events).zip(&placements) {
        if place.band != previous_band {
            for (lane, &live) in active.iter().enumerate() {
                if live {
                    canvas.fold(previous_band, previous_end, lane);
                }
            }
        }
        let top = lanes * 2 + 2;
        for &id in &event.sources {
            let product = &products[id];
            let lane = product.lane.expect("layout indexed a source lane");
            canvas.label(place.band, place.start, lane * 2, product.name, true);
            active[lane] = true;
        }
        for (lane, &live) in active.iter().enumerate() {
            if !live {
                continue;
            }
            let consumed = event
                .inputs
                .iter()
                .position(|&id| products[id].lane == Some(lane) && remaining[id] == 1);
            let end = consumed.map_or(place.end, |port| place.operation + 2 + port * 2);
            let source = event
                .sources
                .iter()
                .find(|&&id| products[id].lane == Some(lane));
            let start = source.map_or(place.start, |&id| {
                place.start + products[id].name.chars().count() + 2
            });
            canvas.line(place.band, (start, lane * 2), (end, lane * 2));
        }
        canvas.operation(place.band, place.operation, top, place.box_width, step.name);
        for (port, &id) in event.inputs.iter().enumerate() {
            let lane = products[id].lane.expect("layout indexed an input lane");
            let x = place.operation + 2 + port * 2;
            canvas.line(place.band, (x, lane * 2), (x, top - 1));
            canvas.arrow(place.band, x, top - 1, '▼');
            remaining[id] -= 1;
            if remaining[id] == 0 {
                active[lane] = false;
            }
        }
        if !event.outputs.is_empty() {
            let bus = place.operation + place.box_width + 1;
            canvas.line(
                place.band,
                (place.operation + place.box_width, top + 1),
                (bus, top + 1),
            );
            for &id in &event.outputs {
                let product = &products[id];
                let lane = product.lane.expect("layout indexed an output lane");
                let y = lane * 2;
                canvas.line(place.band, (bus, top + 1), (bus, y));
                canvas.line(place.band, (bus, y), (bus + 2, y));
                canvas.arrow(place.band, bus + 2, y, '▶');
                canvas.label(place.band, bus + 3, y, product.name, true);
                if remaining[id] != 0 {
                    let start = bus + 3 + product.name.chars().count() + 2;
                    canvas.line(place.band, (start, y), (place.end, y));
                    active[lane] = true;
                }
            }
        }
        previous_band = place.band;
        previous_end = place.end;
    }
    canvas.write(text);
    true
}

impl<'a> Canvas<'a> {
    fn point(&self, band: usize, x: usize, y: usize) -> (usize, usize) {
        (
            self.margin + if band % 2 == 0 { x } else { self.inner - 1 - x },
            band * self.pitch + y,
        )
    }

    fn line(&mut self, band: usize, from: (usize, usize), to: (usize, usize)) {
        self.segment(
            self.point(band, from.0, from.1),
            self.point(band, to.0, to.1),
        );
    }

    fn segment(&mut self, from: (usize, usize), to: (usize, usize)) {
        let (mut x, mut y) = from;
        let (tx, ty) = to;
        while (x, y) != (tx, ty) {
            let (nx, ny, bit, reverse) = if x < tx {
                (x + 1, y, 2, 8)
            } else if x > tx {
                (x - 1, y, 8, 2)
            } else if y < ty {
                (x, y + 1, 4, 1)
            } else {
                (x, y - 1, 1, 4)
            };
            self.edges[y * WIDTH + x] |= bit;
            self.edges[ny * WIDTH + nx] |= reverse;
            x = nx;
            y = ny;
        }
    }

    fn fold(&mut self, band: usize, end: usize, lane: usize) {
        let from = self.point(band, end, lane * 2);
        let to = self.point(band + 1, 0, lane * 2);
        let x = if band % 2 == 0 {
            WIDTH - 2 - lane * 2
        } else {
            1 + lane * 2
        };
        self.segment(from, (x, from.1));
        self.segment((x, from.1), (x, to.1));
        self.segment((x, to.1), to);
    }

    fn arrow(&mut self, band: usize, x: usize, y: usize, arrow: char) {
        let (x, y) = self.point(band, x, y);
        let arrow = if band % 2 == 1 && arrow == '▶' {
            '◀'
        } else {
            arrow
        };
        self.arrows.push((x, y, arrow));
    }

    fn label(&mut self, band: usize, x: usize, y: usize, name: &'a str, product: bool) {
        let width = name.chars().count() + if product { 2 } else { 0 };
        let (x, y) = self.point(band, x + if band % 2 == 1 { width - 1 } else { 0 }, y);
        self.labels.push(Label {
            x,
            y,
            name,
            product,
        });
    }

    fn operation(&mut self, band: usize, x: usize, y: usize, width: usize, name: &'a str) {
        let (left, top) = self.point(band, x + if band % 2 == 1 { width - 1 } else { 0 }, y);
        self.boxes.push((left, top, width));
        self.label(
            band,
            x + (width - name.chars().count()) / 2,
            y + 1,
            name,
            false,
        );
    }

    fn write(self, text: &mut String) {
        let glyphs = [
            ' ', '│', '─', '└', '│', '│', '┌', '├', '─', '┘', '─', '┴', '┐', '┤', '┬', '╪',
        ];
        let mut rows: Vec<Vec<char>> = self
            .edges
            .chunks(WIDTH)
            .map(|row| row.iter().map(|&bits| glyphs[usize::from(bits)]).collect())
            .collect();
        for (x, y, arrow) in self.arrows {
            rows[y][x] = arrow;
        }
        for (x, y, width) in self.boxes {
            rows[y][x..x + width].fill('─');
            rows[y + 2][x..x + width].fill('─');
            rows[y][x] = '┌';
            rows[y][x + width - 1] = '┐';
            rows[y + 2][x] = '└';
            rows[y + 2][x + width - 1] = '┘';
            rows[y + 1][x] = '│';
            rows[y + 1][x + width - 1] = '│';
        }
        for label in self.labels {
            let mut x = label.x;
            if label.product {
                rows[label.y][x] = '[';
                x += 1;
            }
            for c in label.name.chars() {
                rows[label.y][x] = c;
                x += 1;
            }
            if label.product {
                rows[label.y][x] = ']';
            }
        }
        let end = rows
            .iter()
            .rposition(|row| row.iter().any(|&c| c != ' '))
            .map_or(0, |i| i + 1);
        for row in &rows[..end] {
            write_row(row, text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folding_keeps_each_product_on_a_separate_continuous_line() {
        let mut canvas = Canvas {
            edges: vec![0; 24 * WIDTH],
            arrows: Vec::new(),
            labels: Vec::new(),
            boxes: Vec::new(),
            margin: 6,
            inner: 108,
            pitch: 12,
        };
        canvas.fold(0, 80, 0);
        canvas.fold(0, 80, 1);
        let mut text = String::new();
        canvas.write(&mut text);
        assert!(text.contains('╪'));
        assert_eq!(canvas_point(&text, 118, 0), '┐');
        assert_eq!(canvas_point(&text, 118, 12), '┘');
        assert_eq!(canvas_point(&text, 116, 12), '╪');
        assert_eq!(canvas_point(&text, 116, 14), '┘');
    }

    fn canvas_point(text: &str, x: usize, y: usize) -> char {
        text.lines().nth(y).unwrap().chars().nth(x).unwrap()
    }

    #[test]
    fn reversed_bands_keep_labels_readable_and_reverse_horizontal_arrows() {
        let mut canvas = Canvas {
            edges: vec![0; 24 * WIDTH],
            arrows: Vec::new(),
            labels: Vec::new(),
            boxes: Vec::new(),
            margin: 6,
            inner: 108,
            pitch: 12,
        };
        canvas.arrow(1, 4, 0, '▶');
        canvas.label(1, 5, 0, "product", true);
        let mut text = String::new();
        canvas.write(&mut text);
        assert!(text.contains("[product]◀"));
        assert!(!text.contains("tcudorp"));
    }
}
