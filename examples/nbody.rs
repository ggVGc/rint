// N-body simulation (floating point, structs in a Vec, &mut slices).
const PI: f64 = std::f64::consts::PI;
const SOLAR_MASS: f64 = 4.0 * PI * PI;
const DAYS_PER_YEAR: f64 = 365.24;

#[derive(Clone, Copy)]
struct Body {
    pos: [f64; 3],
    vel: [f64; 3],
    mass: f64,
}

fn bodies() -> Vec<Body> {
    vec![
        Body { pos: [0.0; 3], vel: [0.0; 3], mass: SOLAR_MASS },
        Body {
            pos: [4.84143144246472090e+00, -1.16032004402742839e+00, -1.03622044471123109e-01],
            vel: [1.66007664274403694e-03 * DAYS_PER_YEAR, 7.69901118419740425e-03 * DAYS_PER_YEAR, -6.90460016972063023e-05 * DAYS_PER_YEAR],
            mass: 9.54791938424326609e-04 * SOLAR_MASS,
        },
        Body {
            pos: [8.34336671824457987e+00, 4.12479856412430479e+00, -4.03523417114321381e-01],
            vel: [-2.76742510726862411e-03 * DAYS_PER_YEAR, 4.99852801234917238e-03 * DAYS_PER_YEAR, 2.30417297573763929e-05 * DAYS_PER_YEAR],
            mass: 2.85885980666130812e-04 * SOLAR_MASS,
        },
        Body {
            pos: [1.28943695621391310e+01, -1.51111514016986312e+01, -2.23307578892655734e-01],
            vel: [2.96460137564761618e-03 * DAYS_PER_YEAR, 2.37847173959480950e-03 * DAYS_PER_YEAR, -2.96589568540237556e-05 * DAYS_PER_YEAR],
            mass: 4.36624404335156298e-05 * SOLAR_MASS,
        },
        Body {
            pos: [1.53796971148509165e+01, -2.59193146099879641e+01, 1.79258772950371181e-01],
            vel: [2.68067772490389322e-03 * DAYS_PER_YEAR, 1.62824170038242295e-03 * DAYS_PER_YEAR, -9.51592254519715870e-05 * DAYS_PER_YEAR],
            mass: 5.15138902046611451e-05 * SOLAR_MASS,
        },
    ]
}

fn offset_momentum(b: &mut [Body]) {
    let mut p = [0.0; 3];
    for body in b.iter() {
        for k in 0..3 {
            p[k] += body.vel[k] * body.mass;
        }
    }
    for k in 0..3 {
        b[0].vel[k] = -p[k] / SOLAR_MASS;
    }
}

fn energy(b: &[Body]) -> f64 {
    let mut e = 0.0;
    for i in 0..b.len() {
        let bi = &b[i];
        e += 0.5 * bi.mass * (bi.vel[0] * bi.vel[0] + bi.vel[1] * bi.vel[1] + bi.vel[2] * bi.vel[2]);
        for j in (i + 1)..b.len() {
            let bj = &b[j];
            let dx = bi.pos[0] - bj.pos[0];
            let dy = bi.pos[1] - bj.pos[1];
            let dz = bi.pos[2] - bj.pos[2];
            e -= bi.mass * bj.mass / (dx * dx + dy * dy + dz * dz).sqrt();
        }
    }
    e
}

fn advance(b: &mut [Body], dt: f64) {
    let n = b.len();
    for i in 0..n {
        for j in (i + 1)..n {
            let dx = b[i].pos[0] - b[j].pos[0];
            let dy = b[i].pos[1] - b[j].pos[1];
            let dz = b[i].pos[2] - b[j].pos[2];
            let d2 = dx * dx + dy * dy + dz * dz;
            let mag = dt / (d2 * d2.sqrt());
            let mj = b[j].mass * mag;
            let mi = b[i].mass * mag;
            b[i].vel[0] -= dx * mj;
            b[i].vel[1] -= dy * mj;
            b[i].vel[2] -= dz * mj;
            b[j].vel[0] += dx * mi;
            b[j].vel[1] += dy * mi;
            b[j].vel[2] += dz * mi;
        }
    }
    for body in b.iter_mut() {
        for k in 0..3 {
            body.pos[k] += dt * body.vel[k];
        }
    }
}

fn main() {
    let n: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(1000);
    let mut b = bodies();
    offset_momentum(&mut b);
    println!("{:.9}", energy(&b));
    for _ in 0..n {
        advance(&mut b, 0.01);
    }
    println!("{:.9}", energy(&b));
}
