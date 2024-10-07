use core::cmp::Ordering;

use alloc::collections::VecDeque;
use alloc::vec::Vec;

pub struct HuffmanTable {
    /// Index is the symbol, values are the bitstring in the lower bits of the u32 and the amount of bits in the u8
    codes: Vec<(u32, u8)>,
}

impl HuffmanTable {
    pub fn build(weights: &[usize]) -> Self {
        let mut sorted = Vec::with_capacity(weights.len());
        struct SortEntry {
            symbol: u8,
            weight: usize,
        }
        for (symbol, weight) in weights.iter().copied().enumerate() {
            if weight > 0 {
                sorted.push(SortEntry {
                    symbol: symbol as u8,
                    weight,
                });
            }
        }
        sorted.sort_by(|left, right| match left.weight.cmp(&right.weight) {
            Ordering::Equal => left.symbol.cmp(&right.symbol),
            other => other,
        });

        let mut queue1 = VecDeque::new();
        let mut queue2 = VecDeque::new();
        let mut nodes = Vec::new();

        enum Node {
            Leaf {
                entry: SortEntry,
            },
            Internal {
                left: usize,
                right: usize,
                weight: usize,
            },
        }
        impl PartialEq for Node {
            fn eq(&self, other: &Self) -> bool {
                self.weight().eq(&other.weight())
            }
        }
        impl Eq for Node {}
        impl PartialOrd for Node {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                Some(self.weight().cmp(&other.weight()))
            }
        }
        impl Ord for Node {
            fn cmp(&self, other: &Self) -> Ordering {
                self.weight().cmp(&other.weight())
            }
        }
        impl Node {
            fn weight(&self) -> usize {
                match self {
                    Self::Leaf { entry } => entry.weight,
                    Self::Internal { weight, .. } => *weight,
                }
            }
        }
        for (idx, entry) in sorted.into_iter().enumerate() {
            nodes.push(Node::Leaf { entry });
            queue1.push_back(idx);
        }

        loop {
            let min1 = pop_min(&mut queue1, &mut queue2, &nodes);
            let min2 = pop_min(&mut queue1, &mut queue2, &nodes);
            match (min1, min2) {
                (Some(_root), None) | (None, Some(_root)) => {
                    break;
                }
                (None, None) => unreachable!(),
                (Some(left), Some(right)) => {
                    nodes.push(Node::Internal {
                        left,
                        right,
                        weight: nodes[left].weight() + nodes[right].weight(),
                    });
                    queue2.push_back(nodes.len() - 1);
                }
            }
        }
        let mut table = HuffmanTable {
            codes: Vec::with_capacity(weights.len()),
        };
        for _ in weights {
            table.codes.push((0, 0));
        }

        // Todo traverse tree and put codes into the table

        table
    }
}

fn pop_min<T: Ord>(
    q1: &mut VecDeque<usize>,
    q2: &mut VecDeque<usize>,
    elements: &[T],
) -> Option<usize> {
    if q1.is_empty() {
        q2.pop_front()
    } else if q2.is_empty() {
        q1.pop_front()
    } else {
        let e1 = &elements[*q1.front().unwrap()];
        let e2 = &elements[*q2.front().unwrap()];

        match e1.cmp(e2) {
            Ordering::Equal => q1,
            Ordering::Less => q1,
            Ordering::Greater => q2,
        }
        .pop_front()
    }
}
