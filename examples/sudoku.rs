// Backtracking sudoku solver over a fixed-size 2D array passed by &mut.
type Board = [[u8; 9]; 9];

fn parse(s: &str) -> Board {
    let mut b = [[0u8; 9]; 9];
    for (i, c) in s.chars().filter(|c| !c.is_whitespace()).enumerate() {
        b[i / 9][i % 9] = if c == '.' { 0 } else { c.to_digit(10).unwrap() as u8 };
    }
    b
}

fn ok(b: &Board, r: usize, c: usize, v: u8) -> bool {
    for i in 0..9 {
        if b[r][i] == v || b[i][c] == v {
            return false;
        }
    }
    let (br, bc) = (r / 3 * 3, c / 3 * 3);
    for i in 0..3 {
        for j in 0..3 {
            if b[br + i][bc + j] == v {
                return false;
            }
        }
    }
    true
}

fn solve(b: &mut Board, count: &mut u64) -> bool {
    for r in 0..9 {
        for c in 0..9 {
            if b[r][c] == 0 {
                for v in 1..=9 {
                    if ok(b, r, c, v) {
                        b[r][c] = v;
                        *count += 1;
                        if solve(b, count) {
                            return true;
                        }
                        b[r][c] = 0;
                    }
                }
                return false;
            }
        }
    }
    true
}

fn show(b: &Board) {
    for (i, row) in b.iter().enumerate() {
        if i % 3 == 0 && i > 0 {
            println!("------+-------+------");
        }
        let cells: Vec<String> = row.iter().map(|v| v.to_string()).collect();
        println!("{} | {} | {}", cells[0..3].join(" "), cells[3..6].join(" "), cells[6..9].join(" "));
    }
}

fn main() {
    let puzzle = "
        53..7....
        6..195...
        .98....6.
        8...6...3
        4..8.3..1
        7...2...6
        .6....28.
        ...419..5
        ....8..79";
    let mut b = parse(puzzle);
    let original = b;
    let mut count = 0;
    let solved = solve(&mut b, &mut count);
    println!("solved={} placements={}", solved, count);
    show(&b);
    println!("original unchanged: {}", original[0][2] == 0);
}
