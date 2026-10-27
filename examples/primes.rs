// Sieve of Eratosthenes, prime factorization, and modular exponentiation.
fn sieve(n: usize) -> Vec<usize> {
    let mut is = vec![true; n + 1];
    is[0] = false;
    if n >= 1 {
        is[1] = false;
    }
    let mut i = 2;
    while i * i <= n {
        if is[i] {
            let mut j = i * i;
            while j <= n {
                is[j] = false;
                j += i;
            }
        }
        i += 1;
    }
    is.iter().enumerate().filter(|(_, &p)| p).map(|(i, _)| i).collect()
}

fn factorize(mut n: u64) -> Vec<(u64, u32)> {
    let mut out = Vec::new();
    let mut p = 2;
    while p * p <= n {
        let mut k = 0;
        while n % p == 0 {
            n /= p;
            k += 1;
        }
        if k > 0 {
            out.push((p, k));
        }
        p += 1;
    }
    if n > 1 {
        out.push((n, 1));
    }
    out
}

fn mod_pow(mut base: u64, mut exp: u64, m: u64) -> u64 {
    let mut result = 1;
    base %= m;
    while exp > 0 {
        if exp & 1 == 1 {
            result = result * base % m;
        }
        base = base * base % m;
        exp >>= 1;
    }
    result
}

fn main() {
    let n: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(100_000);
    let ps = sieve(n);
    println!("{} primes below {}, last {:?}", ps.len(), n, ps.last());
    println!("first ten: {:?}", &ps[..10]);
    let twin = ps.windows(2).filter(|w| w[1] - w[0] == 2).count();
    println!("twin prime pairs: {}", twin);
    for x in [360u64, 97, 1_000_000_007 * 3, 600851475143] {
        let f = factorize(x);
        let s: Vec<String> = f.iter().map(|(p, k)| if *k > 1 { format!("{}^{}", p, k) } else { p.to_string() }).collect();
        println!("{} = {}", x, s.join(" * "));
    }
    println!("2^100 mod 1e9+7 = {}", mod_pow(2, 100, 1_000_000_007));
    let fermat = ps.iter().take(20).skip(1).all(|&p| mod_pow(2, p as u64 - 1, p as u64) == 1);
    println!("fermat holds for small primes: {}", fermat);
}
