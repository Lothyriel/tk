//! Finite contact-capacity support on actual surviving solid islands. A small
//! residual ligament has finite load capacity even while still connected.
use super::*;
use std::collections::VecDeque;

#[derive(Clone, Copy)]
pub struct Node {
    pub cell: u16,
    pub structure: u16,
    pub surface: SurfaceKind,
    pub solid: SolidBox,
}

#[derive(Clone)]
struct Edge {
    target: usize,
    reverse: usize,
    capacity: f32,
}

fn edge(graph: &mut [Vec<Edge>], a: usize, b: usize, capacity: f32) {
    let ar = graph[a].len();
    let br = graph[b].len();
    graph[a].push(Edge {
        target: b,
        reverse: br,
        capacity,
    });
    graph[b].push(Edge {
        target: a,
        reverse: ar,
        capacity: 0.0,
    });
}

fn augment(
    graph: &mut [Vec<Edge>],
    level: &[i32],
    cursor: &mut [usize],
    node: usize,
    sink: usize,
    limit: f32,
) -> f32 {
    if node == sink {
        return limit;
    }
    while cursor[node] < graph[node].len() {
        let i = cursor[node];
        let e = graph[node][i].clone();
        if e.capacity > 0.0001 && level[e.target] == level[node] + 1 {
            let flow = augment(graph, level, cursor, e.target, sink, limit.min(e.capacity));
            if flow > 0.0001 {
                graph[node][i].capacity -= flow;
                graph[e.target][e.reverse].capacity += flow;
                return flow;
            }
        }
        cursor[node] += 1;
    }
    0.0
}

/// Material which cannot route its own weight through finite-capacity contacts
/// to the floor is released. No original-cell connectivity assumption remains.
pub fn unsupported(nodes: &[Node]) -> Vec<bool> {
    let source = nodes.len();
    let sink = source + 1;
    let mut graph = vec![vec![]; sink + 1];
    for (i, node) in nodes.iter().enumerate() {
        let material = material::properties(node.surface);
        edge(
            &mut graph,
            source,
            i,
            node.solid.volume() * material.density * 9.81,
        );
        if node.solid.center.y - node.solid.half_size.y <= 0.1001 {
            edge(&mut graph, i, sink, 1e9);
        }
        for (j, other) in nodes[..i].iter().enumerate() {
            if node.structure != other.structure {
                continue;
            }
            let (area, axis) = contact(node.solid, other.solid);
            if area <= 1e-9 {
                continue;
            }
            let strength = if axis == 1 {
                material.compression
            } else {
                material.shear
            };
            edge(&mut graph, i, j, area * strength);
            edge(&mut graph, j, i, area * strength);
        }
    }
    loop {
        let mut level = vec![-1; graph.len()];
        level[source] = 0;
        let mut queue = VecDeque::from([source]);
        while let Some(node) = queue.pop_front() {
            for e in &graph[node] {
                if e.capacity > 0.0001 && level[e.target] < 0 {
                    level[e.target] = level[node] + 1;
                    queue.push_back(e.target);
                }
            }
        }
        if level[sink] < 0 {
            return level[..nodes.len()]
                .iter()
                .map(|&level| level >= 0)
                .collect();
        }
        let mut cursor = vec![0; graph.len()];
        while augment(&mut graph, &level, &mut cursor, source, sink, f32::MAX) > 0.0001 {}
    }
}

/// Deterministic adjacent grouping gives irregular, volume-preserving pieces,
/// crossing old cell borders where the remaining solids are connected.
pub fn fragments(nodes: &[Node]) -> Vec<Vec<usize>> {
    let mut seen = vec![false; nodes.len()];
    let mut result = Vec::new();
    for start in 0..nodes.len() {
        if seen[start] {
            continue;
        }
        let limit = 5 + (nodes[start].cell as usize * 7 + start) % 8;
        let mut fragment = vec![start];
        seen[start] = true;
        let mut cursor = 0;
        while cursor < fragment.len() && fragment.len() < limit {
            let i = fragment[cursor];
            for (j, node) in nodes.iter().enumerate() {
                if !seen[j]
                    && node.structure == nodes[i].structure
                    && contact(node.solid, nodes[i].solid).0 > 1e-9
                {
                    seen[j] = true;
                    fragment.push(j);
                    if fragment.len() == limit {
                        break;
                    }
                }
            }
            cursor += 1;
        }
        result.push(fragment);
    }
    result
}
