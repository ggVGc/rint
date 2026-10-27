// Dijkstra with a BinaryHeap and a custom Ord, plus BFS on a grid.
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashSet, VecDeque};

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
struct State {
    cost: u32,
    node: usize,
}

impl Ord for State {
    fn cmp(&self, other: &Self) -> Ordering {
        other.cost.cmp(&self.cost).then_with(|| self.node.cmp(&other.node))
    }
}

impl PartialOrd for State {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

struct Edge {
    to: usize,
    w: u32,
}

fn shortest(adj: &Vec<Vec<Edge>>, start: usize) -> Vec<Option<u32>> {
    let mut dist: Vec<Option<u32>> = vec![None; adj.len()];
    let mut heap = BinaryHeap::new();
    dist[start] = Some(0);
    heap.push(State { cost: 0, node: start });
    while let Some(State { cost, node }) = heap.pop() {
        if let Some(d) = dist[node] {
            if cost > d {
                continue;
            }
        }
        for e in &adj[node] {
            let next = State { cost: cost + e.w, node: e.to };
            let better = match dist[next.node] {
                None => true,
                Some(d) => next.cost < d,
            };
            if better {
                heap.push(next);
                dist[next.node] = Some(next.cost);
            }
        }
    }
    dist
}

fn bfs(grid: &[&str], start: (usize, usize), goal: (usize, usize)) -> Option<usize> {
    let h = grid.len();
    let w = grid[0].len();
    let cells: Vec<Vec<char>> = grid.iter().map(|r| r.chars().collect()).collect();
    let mut seen = HashSet::new();
    let mut q = VecDeque::new();
    q.push_back((start, 0));
    seen.insert(start);
    while let Some(((r, c), d)) = q.pop_front() {
        if (r, c) == goal {
            return Some(d);
        }
        let dirs: [(i32, i32); 4] = [(0, 1), (1, 0), (0, -1), (-1, 0)];
        for (dr, dc) in dirs.iter() {
            let nr = r as i32 + dr;
            let nc = c as i32 + dc;
            if nr < 0 || nc < 0 || nr >= h as i32 || nc >= w as i32 {
                continue;
            }
            let p = (nr as usize, nc as usize);
            if cells[p.0][p.1] == '#' || seen.contains(&p) {
                continue;
            }
            seen.insert(p);
            q.push_back((p, d + 1));
        }
    }
    None
}

fn main() {
    let edges = [(0, 2, 10), (0, 1, 1), (1, 3, 2), (2, 1, 1), (2, 3, 3), (2, 4, 1), (3, 0, 7), (3, 4, 2)];
    let mut adj: Vec<Vec<Edge>> = (0..6).map(|_| Vec::new()).collect();
    for &(a, b, w) in edges.iter() {
        adj[a].push(Edge { to: b, w });
    }
    let d = shortest(&adj, 0);
    for (i, x) in d.iter().enumerate() {
        match x {
            Some(c) => println!("0 -> {}: {}", i, c),
            None => println!("0 -> {}: unreachable", i),
        }
    }
    let mut h = BinaryHeap::new();
    h.push(State { cost: 5, node: 1 });
    h.push(State { cost: 2, node: 7 });
    h.push(State { cost: 9, node: 3 });
    println!("{:?}", h.pop());
    let grid = ["..#.......", ".##.####..", "....#..#..", "#.#.#.##.#", "..#...#...", ".##.#...#."];
    println!("bfs: {:?}", bfs(&grid, (0, 0), (5, 9)));
    println!("bfs blocked: {:?}", bfs(&[".#", "#."], (0, 0), (1, 1)));
}
