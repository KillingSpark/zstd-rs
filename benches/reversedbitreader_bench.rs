use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rand::{Rng, SeedableRng};
use ruzstd::decoding::bit_reader_reverse::BitReaderReversed;

fn do_all_accesses(br: &mut BitReaderReversed, accesses: &[u8]) -> u64 {
    let mut sum = 0;
    for x in accesses {
        sum += br.get_bits(*x).unwrap();
    }
    let _ = black_box(br);
    sum
}

fn criterion_benchmark(c: &mut Criterion) {
    const DATA_SIZE: usize = 1024 * 1024;

    let mut rng = rand::rngs::SmallRng::seed_from_u64(0xDEADBEEF);
    let mut rand_vec = vec![];
    for _ in 0..DATA_SIZE {
        rand_vec.push(rng.gen());
    }

    let mut access_vec = vec![];
    let mut br = BitReaderReversed::new(&rand_vec);
    while br.bits_remaining() > 0 {
        let x = rng.gen_range(0..20);
        br.get_bits(x).unwrap();
        access_vec.push(x);
    }

    c.bench_function("reversed bitreader", |b| {
        b.iter(|| {
            br.reset(&rand_vec);
            do_all_accesses(&mut br, &access_vec)
        })
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
