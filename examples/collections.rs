// Vec, HashMap, HashSet, BTreeMap, BTreeSet, VecDeque, BinaryHeap.
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap, HashSet, VecDeque};

fn main() {
    // Vec
    let mut v: Vec<i32> = Vec::new();
    for i in 0..10 {
        v.push(i * 3 % 7);
    }
    println!("{:?} len={} first={:?} last={:?}", v, v.len(), v.first(), v.last());
    v.sort_unstable();
    v.dedup();
    println!("sorted+dedup {:?}", v);
    v.retain(|&x| x != 3);
    v.insert(1, 100);
    let removed = v.remove(0);
    println!("{:?} removed {} contains 100: {}", v, removed, v.contains(&100));
    v.reverse();
    v.truncate(4);
    v.extend(vec![7, 8]);
    v.swap(0, 1);
    println!("{:?} max={:?} min={:?} pos={:?}", v, v.iter().max(), v.iter().min(), v.iter().position(|&x| x == 7));
    let mut grid = vec![vec![0; 3]; 2];
    grid[1][2] = 5;
    grid[0][0] += 1;
    println!("{:?}", grid);
    let words = vec!["apple", "bob", "cat"];
    let mut by_len = words.clone();
    by_len.sort_by(|a, b| b.len().cmp(&a.len()).then(a.cmp(b)));
    println!("{:?} {}", by_len, words.join("+"));
    println!("{:?}", [3, 1, 2].iter().map(|x| x * 10).collect::<Vec<_>>());
    println!("{:?}", v.windows(2).map(|w| w[0] + w[1]).collect::<Vec<_>>());
    println!("{:?}", v.chunks(4).map(|c| c.to_vec()).collect::<Vec<_>>());
    println!("{:?} {:?}", v.binary_search(&8), vec![1, 3, 5].binary_search(&4));
    let (left, right) = v.split_at(2);
    println!("{:?} {:?}", left, right);

    // HashMap (sorted output for determinism)
    let text = "the quick brown fox jumps over the lazy dog the end";
    let mut freq: HashMap<&str, usize> = HashMap::new();
    for w in text.split(' ') {
        *freq.entry(w).or_insert(0) += 1;
    }
    let mut pairs: Vec<(&str, usize)> = freq.iter().map(|(k, v)| (*k, *v)).collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    println!("{:?}", &pairs[..3]);
    println!("{} {:?} {:?}", freq.len(), freq.get("fox"), freq.get("cow"));
    if let Some(c) = freq.get_mut("dog") {
        *c += 10;
    }
    println!("dog={} has_cat={}", freq["dog"], freq.contains_key("cat"));
    let mut groups: HashMap<usize, Vec<&str>> = HashMap::new();
    for w in text.split_whitespace() {
        groups.entry(w.len()).or_default().push(w);
    }
    let mut lens: Vec<_> = groups.keys().copied().collect();
    lens.sort();
    for l in lens {
        println!("{} -> {:?}", l, groups[&l]);
    }

    // BTreeMap / BTreeSet are ordered
    let mut bt = BTreeMap::new();
    for (i, c) in "hello world".chars().enumerate() {
        bt.entry(c).or_insert_with(Vec::new).push(i);
    }
    println!("{:?}", bt);
    let set: BTreeSet<i32> = [5, 1, 9, 1, 3].into_iter().collect();
    println!("{:?} first={:?}", set, set.iter().next());

    // HashSet operations (sorted for output)
    let a: HashSet<i32> = (1..6).collect();
    let b: HashSet<i32> = (4..9).collect();
    let mut inter: Vec<_> = a.intersection(&b).cloned().collect();
    inter.sort();
    let mut uni: Vec<_> = a.union(&b).cloned().collect();
    uni.sort();
    let mut diff: Vec<_> = a.difference(&b).cloned().collect();
    diff.sort();
    println!("{:?} {:?} {:?}", inter, uni, diff);

    // VecDeque
    let mut dq = VecDeque::new();
    dq.push_back(1);
    dq.push_back(2);
    dq.push_front(0);
    println!("{:?} {:?} {:?}", dq, dq.front(), dq.back());
    while let Some(x) = dq.pop_front() {
        print!("{} ", x);
    }
    println!();

    // BinaryHeap (max-heap) and min-heap via Reverse
    let mut heap = BinaryHeap::new();
    for x in [5, 1, 8, 3, 9, 2] {
        heap.push(x);
    }
    let mut out = vec![];
    while let Some(x) = heap.pop() {
        out.push(x);
    }
    println!("{:?}", out);
    let mut minh = BinaryHeap::new();
    for x in [5, 1, 8] {
        minh.push(Reverse(x));
    }
    println!("{:?}", minh.pop().map(|Reverse(x)| x));
    println!("peek {:?}", minh.peek());
}
