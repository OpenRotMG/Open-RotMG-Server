use super::*;
use rand::SeedableRng;

#[test]
fn every_class_crosses_each_level_boundary_and_stops_growing_at_twenty() {
    let content = Content::load(Path::new("data")).unwrap();
    let mut rng = rand::rngs::StdRng::seed_from_u64(777);
    let thresholds = [
        0, 50, 200, 450, 800, 1250, 1800, 2450, 3200, 4050, 5000, 6050, 7200, 8450, 9800, 11250,
        12800, 14450, 16200, 18050,
    ];
    for (kind, class) in &content.classes {
        let mut ch = Character::fresh(content.get(*kind).unwrap()).unwrap();
        for (index, threshold) in thresholds.iter().enumerate().skip(1) {
            let before = ch.base;
            ch.hp = 1;
            ch.mp = 0;
            let amount = threshold - 1 - ch.exp;
            assert!(!ch.gain_experience(&content, amount, &mut rng).unwrap());
            assert_eq!(ch.level, index as i32);
            assert_eq!((ch.base, ch.hp, ch.mp), (before, 1, 0));
            assert!(ch.gain_experience(&content, 1, &mut rng).unwrap());
            assert_eq!((ch.level, ch.exp), (index as i32 + 1, *threshold));
            for (stat, value) in ch.base.iter().enumerate() {
                let (min, max) = class.growth[stat];
                assert!(*value >= (before[stat] + min).min(class.caps[stat]));
                assert!(*value <= (before[stat] + max).min(class.caps[stat]));
            }
            let effective = ch.effective(&content).unwrap();
            assert_eq!((ch.hp, ch.mp), (effective[0], effective[1]));
        }
        let before = ch.base;
        ch.hp = 1;
        ch.mp = 0;
        assert!(!ch.gain_experience(&content, 1950, &mut rng).unwrap());
        assert_eq!((ch.level, ch.exp), (20, 20000));
        assert_eq!((ch.base, ch.hp, ch.mp), (before, 1, 0));

        let mut capped = Character::fresh(content.get(*kind).unwrap()).unwrap();
        capped.base = class.caps.map(|stat| stat - 1);
        assert!(capped.gain_experience(&content, 18050, &mut rng).unwrap());
        assert_eq!(capped.level, 20);
        for (stat, value) in capped.base.iter().enumerate() {
            assert!((class.caps[stat] - 1..=class.caps[stat]).contains(value));
            if class.growth[stat].0 > 0 {
                assert_eq!(*value, class.caps[stat]);
            }
        }
    }
}
