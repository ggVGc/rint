// Basic output and formatting.
fn main() {
    println!("Hello, world!");
    let name = "rint";
    let version = 1;
    println!("{} v{}", name, version);
    println!("{name} v{version}");
    println!("{0} {1} {0}", "a", "b");
    println!("[{:5}] [{:<5}] [{:>5}] [{:^5}] [{:*^9}]", 42, 42, 42, 42, "mid");
    println!("{:+} {:05} {:x} {:X} {:#x} {:b} {:#b} {:o}", 7, -42, 255, 255, 255, 5, 5, 8);
    println!("{:.3} {:8.2} {:<8.1}| {:e}", 3.14159265, 2.5, 1.25, 1234.5);
    println!("{} {} {} {}", 1.0, 0.1 + 0.2, 1e21, -0.0);
    println!("{:?} {:?} {:?}", 1.0, 1.5e-7, f64::MAX);
    println!("{:?} {:?} {:?}", "quote\"d\n", 'c', '\'');
    let w = 10;
    let p = 2;
    println!("[{:w$.p$}] [{:>1$}] [{:.*}]", 1.23456, 8, 3, 9.87654);
    print!("no newline, ");
    print!("then newline\n");
    eprintln!("this goes to stderr");
    let s = format!("{}-{}", "x", 5);
    println!("{s} {}", s.len());
    println!("{{literal braces}}");
    println!("{:?} {:?}", (1, "two", 3.0), ());
    println!("{:?}", [Some(1), None]);
}
