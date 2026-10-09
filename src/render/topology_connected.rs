//! Compact live-product rails within one stage; crossings never imply joins.
use super::topology::{write_row, Step};
use rustc_hash::FxHashMap;
use std::collections::BTreeSet;

struct Product<'a> {
    name: &'a str,
    remaining: usize,
    lane: Option<usize>,
}

struct Event {
    sources: Vec<usize>,
    inputs: Vec<usize>,
    outputs: Vec<usize>,
}

/// Index and allocate once. Width follows simultaneously live products rather
/// than the number of operations. Names are borrowed from the pipeline.
pub(super) fn render(steps: &[&Step<'_>], text: &mut String) {
    let mut products = Vec::new();
    let mut ids = FxHashMap::default();
    for step in steps {
        for name in step
            .inputs
            .iter()
            .copied()
            .chain(step.outputs.iter().map(String::as_str))
        {
            ids.entry(name).or_insert_with(|| {
                let id = products.len();
                products.push(Product {
                    name,
                    remaining: 0,
                    lane: None,
                });
                id
            });
        }
    }
    for step in steps {
        for input in &step.inputs {
            products[ids[input]].remaining += 1;
        }
    }
    let mut free = BTreeSet::new();
    let mut lanes = 0;
    let mut events = Vec::with_capacity(steps.len());
    for step in steps {
        let mut sources = Vec::new();
        let inputs: Vec<_> = step.inputs.iter().map(|name| ids[name]).collect();
        for &id in &inputs {
            if products[id].lane.is_none() {
                allocate(&mut products[id], &mut free, &mut lanes);
                sources.push(id);
            }
        }
        // Retain input lanes until after output allocation so a routed output
        // cannot overwrite an input that is still entering the operation.
        let outputs: Vec<_> = step.outputs.iter().map(|name| ids[name.as_str()]).collect();
        for &id in &outputs {
            allocate(&mut products[id], &mut free, &mut lanes);
        }
        for &id in &inputs {
            products[id].remaining -= 1;
            if products[id].remaining == 0 {
                free.insert(products[id].lane.expect("input was allocated"));
            }
        }
        for &id in &outputs {
            if products[id].remaining == 0 {
                free.insert(products[id].lane.expect("output was allocated"));
            }
        }
        events.push(Event {
            sources,
            inputs,
            outputs,
        });
    }
    let left = lanes * 3 + 2;
    let label_width = products
        .iter()
        .map(|p| p.name.chars().count() + 2)
        .max()
        .unwrap_or(0);
    let box_width = steps
        .iter()
        .map(|s| s.name.chars().count() + 4)
        .max()
        .unwrap_or(4)
        .max(label_width + 4)
        | 1;
    let center = left + box_width - 2;
    let width = left + box_width;
    let mut active = vec![false; lanes];
    let mut remaining = vec![0; products.len()];
    for event in &events {
        for &id in &event.inputs {
            remaining[id] += 1;
        }
    }
    for (step, event) in steps.iter().zip(events) {
        for id in event.sources {
            let p = &products[id];
            let point = p.lane.expect("source was allocated") * 3;
            let mut row = rails(&active, width);
            route(&mut row, point, left, &active);
            row[point] = '┌';
            label(&mut row, left, &format!("[{}]", p.name));
            write_row(&row, text);
            active[point / 3] = true;
        }
        let mut row = rails(&active, width);
        row[left..width].fill('─');
        row[left] = '┌';
        row[width - 1] = '┐';
        write_row(&row, text);
        let mut row = rails(&active, width);
        row[left] = '│';
        row[width - 1] = '│';
        label(
            &mut row,
            left + (box_width - step.name.chars().count()) / 2,
            step.name,
        );
        write_row(&row, text);
        for id in event.inputs {
            let point = products[id].lane.expect("input was allocated") * 3;
            let mut row = rails(&active, width);
            route(&mut row, point, left, &active);
            remaining[id] -= 1;
            row[point] = if remaining[id] == 0 { '└' } else { '├' };
            row[left - 1] = '▶';
            row[left] = '│';
            row[width - 1] = '│';
            write_row(&row, text);
            if remaining[id] == 0 {
                active[point / 3] = false;
            }
        }
        let mut row = rails(&active, width);
        row[left..width].fill('─');
        row[left] = '└';
        row[width - 1] = '┘';
        if !event.outputs.is_empty() {
            row[center] = '┬';
        }
        write_row(&row, text);
        for (index, &id) in event.outputs.iter().enumerate() {
            let p = &products[id];
            let point = p.lane.expect("output was allocated") * 3;
            let mut row = rails(&active, width);
            row[center] = '│';
            write_row(&row, text);
            let mut row = rails(&active, width);
            // Product labels sit outside the rails. The operation's output
            // branches to each named product before any downstream consumers.
            route(&mut row, point, center, &active);
            row[point] = if remaining[id] == 0 { '◀' } else { '┌' };
            row[center] = if index + 1 == event.outputs.len() {
                '┘'
            } else {
                '┤'
            };
            label(&mut row, left, &format!("[{}]", p.name));
            write_row(&row, text);
            active[point / 3] = remaining[id] != 0;
        }
    }
}

fn allocate(product: &mut Product<'_>, free: &mut BTreeSet<usize>, lanes: &mut usize) {
    let lane = free.pop_first().unwrap_or_else(|| {
        let lane = *lanes;
        *lanes += 1;
        lane
    });
    product.lane = Some(lane);
}

fn rails(active: &[bool], width: usize) -> Vec<char> {
    let mut row = vec![' '; width];
    for (lane, &live) in active.iter().enumerate() {
        if live {
            row[lane * 3] = '│';
        }
    }
    row
}

fn route(row: &mut [char], from: usize, to: usize, active: &[bool]) {
    row[from..=to].fill('─');
    for (lane, &live) in active.iter().enumerate() {
        let point = lane * 3;
        if live && point > from && point < to {
            row[point] = '╪';
        }
    }
}

fn label(row: &mut [char], start: usize, text: &str) {
    for (offset, c) in text.chars().enumerate() {
        row[start + offset] = c;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossing_preserves_an_unrelated_product_line() {
        let mut row = rails(&[true, true, true], 12);
        route(&mut row, 0, 9, &[true, true, true]);
        row[0] = '├';
        row[9] = '▶';
        assert_eq!(row.iter().collect::<String>(), "├──╪──╪──▶  ");
    }
}
