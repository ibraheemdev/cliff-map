#![allow(unstable_name_collisions)]

mod map;
mod raw;

use fxhash::FxBuildHasher;

pub use map::HashMap;

#[test]
fn bench() {
    let map = HashMap::new();
    for i in 0..2000 {
        assert_eq!(map.pin().insert(i, i + 1), None);
    }

    let y = map.pin();
    for i in 0..2000 {
        assert_eq!(y.get(&i), Some(&(i + 1)));
    }
}

#[test]
fn resize() {
    use rand::distributions::Distribution;
    let mut rng = rand::thread_rng();
    let zipf = zipf::ZipfDistribution::new(usize::MAX, 1.08).unwrap();
    let sample = zipf.sample_iter(&mut rng).take(1_usize << 22);

    let map = HashMap::with_hasher(FxBuildHasher::default());
    for i in sample {
        map.pin().insert(i, i + 1);
    }
}

#[test]
fn iter() {
    let map = HashMap::new();
    for i in 0..10_000 {
        assert_eq!(map.pin().insert(i, i + 1), None);
    }

    let v: Vec<_> = (0..10_000).map(|i| (i, i + 1)).collect();
    let mut got: Vec<_> = map.pin().iter().map(|(&k, &v)| (k, v)).collect();
    got.sort();
    assert_eq!(v, got);
}

#[test]
fn basic() {
    let map = HashMap::new();

    assert!(map.pin().get(&100).is_none());
    map.pin().insert(100, 101);
    assert_eq!(map.pin().get(&100), Some(&101));
    map.pin().update(100, |x| x + 2);
    assert_eq!(map.pin().get(&100), Some(&103));

    assert!(map.pin().get(&200).is_none());
    map.pin().insert(200, 202);
    assert_eq!(map.pin().get(&200), Some(&202));

    assert!(map.pin().get(&300).is_none());

    assert_eq!(map.pin().remove(&100), Some(&103));
    assert_eq!(map.pin().remove(&200), Some(&202));
    assert!(map.pin().remove(&300).is_none());

    assert!(map.pin().get(&100).is_none());
    assert!(map.pin().get(&200).is_none());
    assert!(map.pin().get(&300).is_none());

    for i in 0..64 {
        assert_eq!(map.pin().insert(i, i + 1), None);
    }

    for i in 0..64 {
        assert_eq!(map.pin().get(&i), Some(&(i + 1)));
    }

    for i in 0..64 {
        assert_eq!(map.pin().update(i, |i| i - 1), Some(&(i + 1)));
    }

    for i in 0..64 {
        assert_eq!(map.pin().get(&i), Some(&i));
    }

    for i in 0..64 {
        assert_eq!(map.pin().remove(&i), Some(&i));
    }

    for i in 0..64 {
        assert_eq!(map.pin().get(&i), None);
    }

    for i in 0..256 {
        assert_eq!(map.pin().insert(i, i + 1), None);
    }

    for i in 0..256 {
        assert_eq!(map.pin().get(&i), Some(&(i + 1)));
    }
}

#[test]
fn stress() {
    let map = HashMap::<usize, usize>::new();
    let chunk = 1 << 14;

    std::thread::scope(|s| {
        for t in 0..16 {
            let map = &map;

            s.spawn(move || {
                let (start, end) = (chunk * t, chunk * (t + 1));

                for i in start..end {
                    assert_eq!(map.pin().insert(i, i + 1), None);
                }

                for i in start..end {
                    assert_eq!(map.pin().get(&i), Some(&(i + 1)));
                }

                for i in start..end {
                    assert_eq!(map.pin().remove(&i), Some(&(i + 1)));
                }

                for i in start..end {
                    assert_eq!(map.pin().get(&i), None);
                }

                for i in start..end {
                    assert_eq!(map.pin().insert(i, i + 1), None);
                }

                for i in start..end {
                    assert_eq!(map.pin().get(&i), Some(&(i + 1)));
                }

                for (&k, &v) in map.pin().iter() {
                    assert!(k < chunk * 16);
                    assert_eq!(v, k + 1);
                }
            });
        }
    });

    let v: Vec<_> = (0..chunk * 16).map(|i| (i, i + 1)).collect();
    let mut got: Vec<_> = map.pin().iter().map(|(&k, &v)| (k, v)).collect();
    got.sort();
    assert_eq!(v, got);
}
