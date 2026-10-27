// Conway's game of life on a torus with Vec<Vec<bool>>.
#[derive(Clone, PartialEq)]
struct World {
    w: usize,
    h: usize,
    cells: Vec<Vec<bool>>,
}

impl World {
    fn from(rows: &[&str]) -> World {
        let cells: Vec<Vec<bool>> = rows.iter().map(|r| r.chars().map(|c| c == '#').collect()).collect();
        World { w: cells[0].len(), h: cells.len(), cells }
    }
    fn neighbors(&self, r: usize, c: usize) -> usize {
        let mut n = 0;
        for dr in [self.h - 1, 0, 1] {
            for dc in [self.w - 1, 0, 1] {
                if dr == 0 && dc == 0 {
                    continue;
                }
                if self.cells[(r + dr) % self.h][(c + dc) % self.w] {
                    n += 1;
                }
            }
        }
        n
    }
    fn step(&self) -> World {
        let mut next = self.clone();
        for r in 0..self.h {
            for c in 0..self.w {
                let n = self.neighbors(r, c);
                next.cells[r][c] = matches!((self.cells[r][c], n), (true, 2) | (true, 3) | (false, 3));
            }
        }
        next
    }
    fn alive(&self) -> usize {
        self.cells.iter().map(|row| row.iter().filter(|&&c| c).count()).sum()
    }
    fn render(&self) -> String {
        self.cells.iter().map(|row| row.iter().map(|&c| if c { '#' } else { '.' }).collect::<String>()).collect::<Vec<_>>().join("\n")
    }
}

fn main() {
    let mut w = World::from(&[
        "..........",
        "...#......",
        "....#.....",
        "..###.....",
        "..........",
        "..........",
        "......##..",
        "......##..",
    ]);
    println!("{}\nalive={}\n", w.render(), w.alive());
    for gen in 1..=20 {
        w = w.step();
        if gen % 10 == 0 {
            println!("gen {}:\n{}\nalive={}\n", gen, w.render(), w.alive());
        }
    }
    let still = World::from(&["....", ".##.", ".##.", "...."]);
    println!("block is still life: {}", still.step() == still);
}
