// Recursion, loops and integer arithmetic.
fn fib(n: u64) -> u64 {
    if n < 2 { n } else { fib(n - 1) + fib(n - 2) }
}

fn fib_iter(n: u64) -> u64 {
    let (mut a, mut b) = (0u64, 1u64);
    for _ in 0..n {
        let t = a + b;
        a = b;
        b = t;
    }
    a
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

fn collatz_len(mut n: u64) -> u32 {
    let mut steps = 0;
    while n != 1 {
        n = if n % 2 == 0 { n / 2 } else { 3 * n + 1 };
        steps += 1;
    }
    steps
}

fn main() {
    for i in [0, 1, 2, 10, 25] {
        println!("fib({}) = {} = {}", i, fib(i), fib_iter(i));
    }
    println!("fib_iter(90) = {}", fib_iter(90));
    println!("gcd(1071, 462) = {}", gcd(1071, 462));
    let (best, len) = (1..10000u64).map(|n| (n, collatz_len(n))).max_by_key(|&(_, l)| l).unwrap();
    println!("longest collatz below 10000: {} ({} steps)", best, len);
    let mut factorial: u64 = 1;
    let mut i = 1;
    let result = loop {
        factorial *= i;
        if factorial > 1_000_000 {
            break i;
        }
        i += 1;
    };
    println!("first n with n! > 1e6: {} ({})", result, factorial);
    'outer: for a in 1..20 {
        for b in a..20 {
            for c in b..30 {
                if a * a + b * b == c * c && a + b + c == 24 {
                    println!("triple: {} {} {}", a, b, c);
                    break 'outer;
                }
            }
        }
    }
    println!("{} {} {}", -7 / 2, -7 % 2, (-7i32).rem_euclid(2));
    println!("{} {} {}", 2i32.pow(10), i32::MAX, u8::MAX);
    println!("{} {}", 255u8 as i8, -1i32 as u8);
    println!("{} {} {}", 3.99f64 as i32, -3.99f64 as i32, 300.0f64 as u8);
    println!("{} {}", 'A' as u8 + 2, (b'a' + 1) as char);
    println!("{}", 10i64.checked_div(0).is_none());
}
