use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rand::Rng;
use ruzstd::decoding::bit_reader_reverse::BitReaderReversed;

fn fibonacci(br: &mut BitReaderReversed, accesses: &[u8]) -> u64 {
    let mut sum = 0;
    for x in accesses {
        sum += br.get_bits(*x).unwrap() as u64;
    }
    let _ = black_box(br);
    sum
}

fn criterion_benchmark(c: &mut Criterion) {
    let mut rng = rand::thread_rng();
    let mut rand_vec = vec![];
    for _ in 0..100000 {
        rand_vec.push(rng.gen());
    }

    let mut access_vec = vec![];
    let mut br = BitReaderReversed::new(&rand_vec);
    while br.bits_remaining() > 0 {
        let x = rng.gen_range(1..20);
        br.get_bits(x).unwrap();
        access_vec.push(x);
    }

    c.bench_function("fib 20", |b| {
        b.iter(|| {
            br.reset(&rand_vec);
            fibonacci(&mut br, &access_vec)
        })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
