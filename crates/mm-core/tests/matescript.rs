use mm_core::matescript::{Bound, Item, Level, Mode, Parser, Point, Settings};
use proptest::prelude::*;

fn settings(calm: bool) -> Settings {
    Settings {
        language: "en".into(),
        mode: if calm { Mode::Calm } else { Mode::Fun },
        point_ids: vec![
            "e3".into(),
            "e4".into(),
            "e8".into(),
            "e12".into(),
            "e17".into(),
            "e21".into(),
        ],
        screenshot: Some((1280, 720)),
    }
}

// Every numbered row of docs/matescript-cases.md, including its expected speech.
fn case(number: usize) -> (&'static str, &'static str) {
    match number {
        1 => ("<lang en>3 < 5 is true.", "3 < 5 is true."),
        2 => ("<lang en><mood happy>Hello.", "Hello."),
        3 => ("<lang en>Hello <sfx ping", "Hello"),
        4 => (
            "<lang en><point aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa>Hi.",
            "Hi.",
        ),
        5 => ("<lang en><sfx birds\nsoft>Morning.", "Morning."),
        6 => ("<lang en><foo bar>Welcome.", "Welcome."),
        7 => ("<lang en><sfx thundr>Storm coming.", "Storm coming."),
        8 => ("<lang en><sfx boom><sfx knock loud>Bang!", "Bang!"),
        9 => (
            "<lang en><sfx ping><sfx pop><sfx boing><sfx tada><sfx cat>Done.",
            "Done.",
        ),
        10 => ("<lang en>Version 1.5 is out.", "Version 1.5 is out."),
        11 => ("<lang en>Dr. Smith arrived.", "Dr. Smith arrived."),
        12 => (
            "<lang en>Visit example.com for help.",
            "Visit example.com for help.",
        ),
        13 => ("<lang en>Wait… What?", "Wait… What?"),
        14 => ("<lang en>\"Done!\" she said.", "\"Done!\" she said."),
        15 => ("<lang ru>Это, т.е., пример.", "Это, т.е., пример."),
        16 => (
            "<lang ja>こんにちは。元気ですか？",
            "こんにちは。 元気ですか？",
        ),
        17 => ("<lang en><point e99>Look here.", "Look here."),
        18 => ("<lang en><point 99999,5>See it.", "See it."),
        19 => (
            "<lang en><amb rain soft><amb forest soft>Birds sing.",
            "Birds sing.",
        ),
        20 => ("<lang en><pause 99999>Ready.", "Ready."),
        21 => ("<lang en><mood happy><end>", ""),
        22 => ("<lang en><sfx tada><sfx pop>", ""),
        23 => ("<lang en>Hello world", "Hello world"),
        24 => (
            "<lang en>Hello.<wait click>How are you?",
            "Hello. How are you?",
        ),
        25 => (
            "<lang en><sfx rimshot solo>That's funny.<sfx success>Nice.",
            "That's funny. Nice.",
        ),
        _ => panic!("unknown case"),
    }
}

fn parse(input: &str, calm: bool) -> Vec<Item> {
    parse_with_settings(input, settings(calm))
}

fn parse_with_settings(input: &str, settings: Settings) -> Vec<Item> {
    let mut parser = Parser::new(settings);
    let mut timeline = parser.push(input);
    timeline.extend(parser.finish());
    timeline
}

fn speech(timeline: &[Item]) -> Vec<&str> {
    timeline
        .iter()
        .filter_map(|item| match item {
            Item::Say(say) => Some(say.text.as_str()),
            _ => None,
        })
        .collect()
}

// Render speech explicitly: production Say::Debug intentionally hides transcripts.
fn golden(timeline: &[Item]) -> String {
    timeline
        .iter()
        .map(|item| match item {
            Item::Say(say) => format!("Say#{} {:?} {:?}", say.id, say.text, say.bound),
            other => format!("{other:?}"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

macro_rules! golden_case {
    ($name:ident, $number:literal) => {
        #[test]
        fn $name() {
            let (input, expected) = case($number);
            let timeline = parse(input, $number == 8);
            assert_eq!(speech(&timeline).join(" "), expected);
            let count = match $number {
                13 | 16 | 24 | 25 => 2,
                21 | 22 => 0,
                _ => 1,
            };
            assert_eq!(speech(&timeline).len(), count);
            insta::assert_snapshot!(golden(&timeline));
        }
    };
}

golden_case!(case_01, 1);
golden_case!(case_02, 2);
golden_case!(case_03, 3);
golden_case!(case_04, 4);
golden_case!(case_05, 5);
golden_case!(case_06, 6);
golden_case!(case_07, 7);
golden_case!(case_08, 8);
golden_case!(case_09, 9);
golden_case!(case_10, 10);
golden_case!(case_11, 11);
golden_case!(case_12, 12);
golden_case!(case_13, 13);
golden_case!(case_14, 14);
golden_case!(case_15, 15);
golden_case!(case_16, 16);
golden_case!(case_17, 17);
golden_case!(case_18, 18);
golden_case!(case_19, 19);
golden_case!(case_20, 20);
golden_case!(case_21, 21);
golden_case!(case_22, 22);
golden_case!(case_23, 23);
golden_case!(case_24, 24);
golden_case!(case_25, 25);

// The four complete examples in docs/matescript.md, section 3.
fn example(number: usize) -> (&'static str, Settings) {
    let input = match number {
        1 => {
            r#"<lang en><mood calm><amb forest soft>Once upon a time, a little fox lived at the edge of a quiet forest.
<sfx birds soft>Every morning the birds woke him up.
One day the sky turned dark. <sfx thunder solo><mood surprised>"Oh no," said the fox, "a storm!"
He ran home just in time, <amb rain soft>and listened to the rain from his warm bed. <mood sleepy>
"#
        }
        2 => {
            r#"<lang en><mood happy><point e12><sfx ping soft>It's right here, the little disk at the top left.
You can also press Control and S together.
"#
        }
        3 => {
            r#"<lang ru><mood calm>Сейчас покажу, это три шага.
<step 1/3 e3>Нажмите «Специальные возможности» слева. <wait click>
<step 2/3 e8>Теперь нажмите «Размер текста». <wait click>
<step 3/3 e21>Передвиньте ползунок вправо и нажмите «Применить». <wait click>
<sfx success soft><mood proud>Готово! Текст стал крупнее.
"#
        }
        4 => {
            r#"<lang en><mood happy>Why did the computer go to the doctor?
Because it had a virus! <sfx rimshot solo><mood proud>
"#
        }
        _ => panic!("unknown example"),
    };
    // Fairy tale and rimshot use fun-mode sounds; the UI examples use calm mode.
    (input, settings(matches!(number, 2 | 3)))
}

macro_rules! golden_example {
    ($name:ident, $number:literal) => {
        #[test]
        fn $name() {
            let (input, settings) = example($number);
            let timeline = parse_with_settings(input, settings);
            insta::assert_snapshot!(golden(&timeline));
        }
    };
}

golden_example!(example_fairy_tale, 1);
golden_example!(example_save_button, 2);
golden_example!(example_three_step_guide, 3);
golden_example!(example_joke_with_rimshot, 4);

fn chunked(input: &str, sizes: &[usize], calm: bool) -> Vec<Item> {
    chunked_with_settings(input, sizes, settings(calm))
}

fn chunked_with_settings(input: &str, sizes: &[usize], settings: Settings) -> Vec<Item> {
    let chars: Vec<char> = input.chars().collect();
    let mut parser = Parser::new(settings);
    let mut timeline = Vec::new();
    let mut offset = 0;
    for size in sizes.iter().cycle() {
        if offset == chars.len() {
            break;
        }
        let end = (offset + size).min(chars.len());
        timeline.extend(parser.push(&chars[offset..end].iter().collect::<String>()));
        timeline.extend(parser.push(""));
        offset = end;
    }
    timeline.extend(parser.finish());
    assert!(parser.finish().is_empty());
    assert!(parser.push("ignored").is_empty());
    timeline
}

proptest! {
    #[test]
    fn every_case_is_chunk_invariant(sizes in prop::collection::vec(1usize..=20, 1..100)) {
        for number in 1..=25 {
            let (input, _) = case(number);
            prop_assert_eq!(chunked(input, &sizes, number == 8), parse(input, number == 8));
        }
    }

    #[test]
    fn every_example_is_chunk_invariant(sizes in prop::collection::vec(1usize..=20, 1..100)) {
        for number in 1..=4 {
            let (input, settings) = example(number);
            prop_assert_eq!(
                chunked_with_settings(input, &sizes, settings.clone()),
                parse_with_settings(input, settings),
            );
        }
    }

    #[test]
    fn arbitrary_unicode_never_panics_and_is_chunk_invariant(
        input in prop::collection::vec(any::<char>(), 0..1000),
        sizes in prop::collection::vec(1usize..=20, 1..100),
    ) {
        let input: String = input.into_iter().collect();
        prop_assert_eq!(chunked(&input, &sizes, false), parse(&input, false));
    }
}

#[test]
fn streaming_and_punctuation() {
    let mut parser = Parser::new(settings(false));
    assert!(parser.push("Hello!").is_empty());
    assert!(parser.push(" ").is_empty());
    assert_eq!(speech(&parser.push("Next ")), ["Hello!"]);
    assert_eq!(speech(&parser.finish()), ["Next"]);
    for input in [
        "Dr. Smith met J. Jones.",
        "Mail a@b.com or use 3.14.",
        "e.g. This stays together.",
        "approx. five",
        "\"Done!\" she said.",
    ] {
        assert_eq!(speech(&parse(input, false)), [input]);
    }
    for input in ["Wait... What?", "Wait… What?", "One.) Two.", "One.\nnext"] {
        assert_eq!(speech(&parse(input, false)).len(), 2, "{input}");
    }
    // A point right before <wait click> is shown before the wait, not after it.
    let timeline = parse("<point e17><wait click>Good.", false);
    assert_eq!(
        timeline[1..3],
        [
            Item::Bound(Bound::Point(Point::Id("e17".into()))),
            Item::Wait
        ]
    );
    assert_eq!(speech(&timeline), ["Good."]);
    // Full-width closing brackets stay with their sentence.
    assert_eq!(
        speech(&parse("「こんにちは。」次です。", false)),
        ["「こんにちは。」", "次です。"]
    );
}

#[test]
fn validation_and_binding() {
    let timeline = parse(
        "<mood happy><mood proud><point e17><point 1279,719><step 1/3 e4><show 你好 125%><pause 1>Go.<wait click><amb off><end>ignored",
        false,
    );
    assert_eq!(timeline[0], Item::Lang("en".into()));
    assert!(timeline.contains(&Item::Pause(100)));
    let say = timeline
        .iter()
        .find_map(|item| {
            if let Item::Say(say) = item {
                Some(say)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(say.bound.len(), 4);
    assert_eq!(say.bound[0], Bound::Mood("proud".into()));
    assert_eq!(say.bound[1], Bound::Point(Point::Pixel(1279, 719)));
    assert_eq!(say.bound[3], Bound::Show("你好 125%".into()));
    assert!(matches!(timeline.last(), Some(Item::End)));
    assert_eq!(speech(&timeline), ["Go."]);

    let calm = parse("<sfx cat normal><sfx knock loud><sfx ping>Hi.", true);
    let bound = calm
        .iter()
        .find_map(|item| {
            if let Item::Say(say) = item {
                Some(&say.bound)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(bound.len(), 2);
    assert!(
        bound
            .iter()
            .all(|b| matches!(b, Bound::Sfx(s) if s.level == Level::Soft))
    );
    assert_eq!(
        speech(&parse("hel<invented tag>lo <point e99>there<", false)),
        ["hello there"]
    );
}

#[test]
fn limits_and_recovery_are_character_based() {
    for input in ["界".repeat(10_000), "word, ".repeat(1000), "x".repeat(221)] {
        let timeline = parse(&input, false);
        assert!(speech(&timeline).iter().all(|s| s.chars().count() <= 220));
        assert_eq!(chunked(&input, &[1, 20, 3], false), timeline);
    }
    let at_limit = format!("<{}>After.", "a".repeat(64));
    assert_eq!(speech(&parse(&at_limit, false)), ["After."]);
    let bounded_discard = format!("<{}{}After.", "a".repeat(64), "x".repeat(256));
    assert_eq!(speech(&parse(&bounded_discard, false)), ["After."]);
    assert_eq!(
        speech(&parse("Before\n<sfx bad\nignored>After.", false)),
        ["Before", "After."]
    );
}

#[test]
fn refusal_and_numeric_failure_rules() {
    let mut parser = Parser::new(settings(false));
    parser.push("<point e99>");
    assert!(parser.needs_point_refusal());
    parser.push("<point e17><point 1280,0>");
    parser.finish();
    assert!(!parser.needs_point_refusal());
    assert!(!Parser::default().needs_point_refusal());

    let timeline = parse(
        "<pause -500><pause +1><pause 99999999999999999999999999999999>",
        false,
    );
    assert_eq!(
        timeline,
        [
            Item::Lang("en".into()),
            Item::Pause(100),
            Item::Pause(100),
            Item::Pause(3000)
        ]
    );
    for input in [
        "<sfx zzzzzzz><mood zzzzzzz>Hi.",
        "<sfx ping solo extra>Hi.",
        "<point 0,720>Hi.",
        "<step 0/3 e4>Hi.",
        "<amb ocean>Hi.",
    ] {
        let timeline = parse(input, false);
        assert_eq!(speech(&timeline), ["Hi."]);
        assert!(timeline.iter().any(|item| matches!(item, Item::BadTag)));
    }
    let fuzzy_mood = parse("<mood hapy>Hi.", false);
    assert!(
        fuzzy_mood.iter().any(
            |item| matches!(item, Item::Say(say) if say.bound == [Bound::Mood("happy".into())])
        )
    );
}
