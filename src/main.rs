use std::io::{BufWriter, Write};

fn usage() -> ! {
    eprintln!("usage: rint <file.rs> [args...]");
    std::process::exit(2);
}

fn main() {
    let mut argv = std::env::args().skip(1);
    let Some(path) = argv.next() else { usage() };
    if path == "-h" || path == "--help" {
        usage();
    }
    let src = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("rint: cannot read {}: {}", path, e);
            std::process::exit(2);
        }
    };
    let mut args = vec![path.clone()];
    args.extend(argv);
    let code = std::thread::Builder::new()
        .stack_size(rint::STACK_SIZE)
        .spawn(move || {
            let out: Box<dyn Write> = Box::new(BufWriter::with_capacity(1 << 16, std::io::stdout()));
            match rint::run(&src, &path, args, out, None) {
                Ok(code) => code,
                Err(msg) => {
                    eprintln!("error: {}", msg);
                    1
                }
            }
        })
        .expect("failed to spawn interpreter thread")
        .join()
        .unwrap_or(101);
    std::process::exit(code);
}
