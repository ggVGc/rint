// String processing.
fn is_palindrome(s: &str) -> bool {
    let cleaned: Vec<char> = s.chars().filter(|c| c.is_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect();
    cleaned.iter().eq(cleaned.iter().rev())
}

fn caesar(s: &str, shift: u8) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_lowercase() {
                (((c as u8 - b'a' + shift) % 26) + b'a') as char
            } else if c.is_ascii_uppercase() {
                (((c as u8 - b'A' + shift) % 26) + b'A') as char
            } else {
                c
            }
        })
        .collect()
}

fn word_lengths(s: &str) -> Vec<(String, usize)> {
    s.split_whitespace().map(|w| (w.to_uppercase(), w.len())).collect()
}

fn main() {
    let mut s = String::from("Hello");
    s.push(',');
    s.push_str(" World");
    s += "!";
    println!("{} len={} upper={} lower={}", s, s.len(), s.to_uppercase(), s.to_lowercase());
    println!("{} {} {}", s.contains("World"), s.starts_with("Hell"), s.ends_with('?'));
    println!("{:?} {:?}", s.find("o"), s.rfind('o'));
    println!("{}", s.replace("l", "L"));
    println!("[{}] [{}] [{}]", "  pad  ".trim(), "  pad  ".trim_start(), "xxpadxx".trim_matches('x'));
    println!("{:?}", "a,b,,c".split(',').collect::<Vec<_>>());
    println!("{:?}", "k=v=w".split_once('='));
    println!("{:?}", "line1\nline2\r\nline3".lines().collect::<Vec<_>>());
    println!("{}", &s[0..5]);
    println!("{:?}", s.char_indices().filter(|(_, c)| c.is_uppercase()).collect::<Vec<_>>());
    for p in ["A man, a plan, a canal: Panama", "hello"] {
        println!("{:?} palindrome? {}", p, is_palindrome(p));
    }
    let enc = caesar("Hello, World!", 3);
    println!("{} -> {}", enc, caesar(&enc, 23));
    println!("{:?}", word_lengths("the quick brown fox"));
    let num = 1234567;
    let digits: Vec<u32> = num.to_string().chars().map(|c| c.to_digit(10).unwrap()).collect();
    println!("{:?} sum={}", digits, digits.iter().sum::<u32>());
    let csv = "name,age\nalice,30\nbob,25";
    let mut total_age = 0;
    for line in csv.lines().skip(1) {
        let fields: Vec<&str> = line.split(',').collect();
        let age: u32 = fields[1].parse().unwrap();
        total_age += age;
        println!("{:<6}|{:>4}", fields[0], age);
    }
    println!("total age {}", total_age);
    let reversed: String = "stressed".chars().rev().collect();
    println!("{}", reversed);
    let bytes = "AZ".as_bytes();
    println!("{:?} {}", bytes, String::from_utf8(vec![82, 117, 115, 116]).unwrap());
    let mut words: Vec<&str> = "banana Apple cherry".split(' ').collect();
    words.sort();
    println!("{:?}", words);
    words.sort_by_key(|w| w.to_lowercase());
    println!("{:?}", words);
    let title: String = "hello wide world".split(' ').map(|w| {
        let mut c = w.chars();
        match c.next() {
            Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            None => String::new(),
        }
    }).collect::<Vec<_>>().join(" ");
    println!("{}", title);
    println!("{}", "ab".repeat(3));
    println!("{:?} {:?}", "42".parse::<u8>(), "300".parse::<u8>().is_err());
    println!("{:?}", "3.5".parse::<f64>().map(|x| x * 2.0));
    let ch = 'ß';
    println!("{} {} {}", ch.len_utf8(), "naïve".len(), "naïve".chars().count());
    println!("{}", 'a'.is_alphabetic() && '9'.is_numeric() && ' '.is_whitespace());
    let mut acc = String::new();
    for (i, w) in ["x", "y", "z"].iter().enumerate() {
        if i > 0 {
            acc.push_str(", ");
        }
        acc += w;
    }
    println!("{}", acc);
    println!("{}", format!("{:>width$}", "right", width = 10));
    println!("{}", char::from_digit(7, 10).unwrap());
    println!("{:?}", "Mississippi".matches("ss").count());
}
