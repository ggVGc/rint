// Closures and iterator adapters.
fn apply<F: Fn(i32) -> i32>(f: F, x: i32) -> i32 {
    f(x)
}

fn make_adder(n: i32) -> impl Fn(i32) -> i32 {
    move |x| x + n
}

fn make_counter() -> impl FnMut() -> u32 {
    let mut count = 0;
    move || {
        count += 1;
        count
    }
}

fn compose<A, B, C>(f: impl Fn(A) -> B, g: impl Fn(B) -> C) -> impl Fn(A) -> C {
    move |x| g(f(x))
}

fn main() {
    let double = |x| x * 2;
    let add5 = make_adder(5);
    println!("{} {} {}", apply(double, 4), add5(10), apply(&add5, 1));
    let mut counter = make_counter();
    counter();
    counter();
    println!("counter = {}", counter());
    let inc_then_double = compose(|x: i32| x + 1, |x: i32| x * 2);
    println!("{}", inc_then_double(3));

    let mut total = 0;
    (1..=4).for_each(|x| total += x);
    println!("total {}", total);

    let v = vec![3, 1, 4, 1, 5, 9, 2, 6, 5, 3];
    let evens: Vec<i32> = v.iter().filter(|&&x| x % 2 == 0).cloned().collect();
    let squares: Vec<i32> = v.iter().map(|x| x * x).collect();
    println!("{:?} {:?}", evens, squares);
    println!("sum={} prod={} count={}", v.iter().sum::<i32>(), v.iter().take(4).product::<i32>(), v.iter().filter(|x| **x > 3).count());
    println!("{:?}", v.iter().enumerate().filter(|(i, _)| i % 3 == 0).map(|(_, x)| *x).collect::<Vec<_>>());
    println!("{:?}", v.iter().skip(2).step_by(3).collect::<Vec<_>>());
    println!("{:?}", v.iter().rev().take_while(|&&x| x != 9).collect::<Vec<_>>());
    println!("{:?}", v.iter().skip_while(|&&x| x < 5).collect::<Vec<_>>());
    println!("{}", v.iter().fold(String::new(), |acc, x| acc + &x.to_string()));
    println!("{:?} {:?}", v.iter().find(|&&x| x > 4), v.iter().find(|&&x| x > 40));
    println!("{} {}", v.iter().any(|&x| x == 9), v.iter().all(|&x| x > 0));
    println!("{:?}", v.iter().zip(v.iter().skip(1)).map(|(a, b)| b - a).collect::<Vec<_>>());
    println!("{:?}", (1..4).flat_map(|x| (0..x).map(move |y| x * 10 + y)).collect::<Vec<_>>());
    println!("{:?}", vec![vec![1, 2], vec![], vec![3]].into_iter().flatten().collect::<Vec<_>>());
    println!("{:?}", ["1", "x", "3"].iter().filter_map(|s| s.parse::<i32>().ok()).collect::<Vec<_>>());
    println!("{:?}", (1..=5).scan(0, |acc, x| { *acc += x; Some(*acc) }).collect::<Vec<_>>());
    let (small, big): (Vec<i32>, Vec<i32>) = v.iter().partition(|&&x| x < 4);
    println!("{:?} {:?}", small, big);
    println!("{:?} {:?}", v.iter().max_by_key(|&&x| (x as i32 - 4).abs()), v.iter().min_by(|a, b| a.cmp(b)));
    let mut it = v.iter().peekable();
    let mut runs = Vec::new();
    while let Some(&x) = it.next() {
        let mut len = 1;
        while let Some(&&y) = it.peek() {
            if y > x { len += 1; it.next(); } else { break; }
        }
        runs.push(len);
    }
    println!("runs {:?}", runs);
    println!("{:?}", (0..3).chain(7..9).rev().collect::<Vec<_>>());
    println!("{:?}", "a-b-c".split('-').map(String::from).collect::<Vec<String>>());
    println!("{:?}", std::iter::successors(Some(1u32), |x| if *x < 100 { Some(x * 3) } else { None }).collect::<Vec<_>>());
    println!("{:?}", std::iter::repeat("ab").take(3).collect::<String>());
    let mut fns: Vec<Box<dyn Fn(i32) -> i32>> = Vec::new();
    for k in 1..=3 {
        fns.push(Box::new(move |x| x * k));
    }
    println!("{:?}", fns.iter().map(|f| f(10)).collect::<Vec<_>>());
    let mut names = vec!["delta".to_string(), "Alpha".into(), "charlie".into(), "Bravo".into()];
    names.sort_by_key(|s| s.to_lowercase());
    println!("{:?}", names);
    let last_word_len = names.last().map(|s| s.len()).unwrap_or(0);
    println!("{}", last_word_len);
}
