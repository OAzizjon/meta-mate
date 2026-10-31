use mm_core::api::intent::{Intent, guess_intent};
use mm_core::matescript::{APPS, Item, OpenTarget, Parser};

#[test]
fn only_unambiguous_https_dns_addresses_are_valid() {
    for url in [
        "https://www.youtube.com",
        "https://example.com/a?q=x&b=2#top",
        "https://Sub.Example.com/path%20name",
    ] {
        assert!(
            matches!(OpenTarget::parse(url), Some(OpenTarget::Site(_))),
            "{url}"
        );
    }
    for url in [
        "http://example.com",
        "https://user:pass@example.com",
        "https://x@example.com",
        "https://127.0.0.1",
        "https://127.1",
        "https://[::1]",
        "https://localhost",
        "https://a.localhost",
        "https://xn--e1afmkfd.xn--p1ai",
        "https://XN--test.com",
        "https://пример.рф",
        "https://example.com/a b",
        "https://example.com/\n",
        "https://example.com/\"",
        "https://example.com/'",
        "https://example.com\\evil",
        "https://example%2ecom",
        "https://example..com",
        "https://-example.com",
        "https://example.com:443",
        "file:///calc.exe",
        "https://example.0x7f",
    ] {
        assert!(OpenTarget::parse(url).is_none(), "{url}");
    }
    let max = format!("https://example.com/{}", "a".repeat(180));
    assert_eq!(max.len(), 200);
    assert!(OpenTarget::parse(&max).is_some());
    assert!(OpenTarget::parse(&(max + "a")).is_none());
}

#[test]
fn app_targets_are_only_from_the_table() {
    assert_eq!(APPS.len(), 11);
    for &(id, name, target, key) in APPS {
        assert_eq!(
            OpenTarget::parse(&format!("app:{name}")),
            Some(OpenTarget::App(id))
        );
        assert_eq!(id.spec(), (target, key));
    }
    for name in [
        "browser",
        "calc.exe",
        "Calculator",
        "paint --evil",
        "unknown",
    ] {
        assert!(OpenTarget::parse(&format!("app:{name}")).is_none());
    }
}

fn parse(text: &str) -> Vec<Item> {
    parse_with(text, Some("open paint, notepad or example.com"))
}

fn parse_with(text: &str, open_request: Option<&str>) -> Vec<Item> {
    let mut parser = Parser::new(mm_core::matescript::Settings {
        open_request: open_request.map(str::to_owned),
        ..Default::default()
    });
    let mut items = Vec::new();
    for ch in text.chars() {
        items.extend(parser.push(&ch.to_string()));
    }
    items.extend(parser.finish());
    items
}

#[test]
fn open_follows_announcement_once_and_never_becomes_speech() {
    for text in [
        "<open app:paint>Opening Paint.<open app:notepad>",
        "Opening Paint.<open app:paint><open app:notepad>",
    ] {
        let items = parse(text);
        assert!(matches!(
            items.as_slice(),
            [Item::Lang(_), Item::Say(_), Item::Open(_)]
        ));
        let Item::Say(say) = &items[1] else {
            panic!("missing sentence")
        };
        assert_eq!(say.text, "Opening Paint.");
        assert_eq!(
            items[2],
            Item::Open(OpenTarget::parse("app:paint").unwrap())
        );
    }
    assert!(
        !parse("<open app:paint>")
            .iter()
            .any(|item| matches!(item, Item::Open(_)))
    );
    let items = parse("<open http://evil.com><open app:unknown>Sorry.");
    assert!(!items.iter().any(|item| matches!(item, Item::Open(_))));
    assert!(
        items
            .iter()
            .any(|item| matches!(item, Item::Say(say) if say.text == "Sorry."))
    );
    let url = format!("https://example.com/{}", "a".repeat(180));
    assert!(
        parse_with(
            &format!("Opening.<open {url}>"),
            Some(&format!("open {url}"))
        )
        .iter()
        .any(|item| matches!(item, Item::Open(_)))
    );
}

#[test]
fn opening_wire_format_matches_the_webview() {
    let target = OpenTarget::parse("app:paint").unwrap();
    assert_eq!(
        serde_json::to_value(Item::Open(target)).unwrap(),
        serde_json::json!({"kind": "open", "value": {"kind": "app", "value": "paint"}})
    );
    let target = OpenTarget::parse("https://example.com/path").unwrap();
    assert_eq!(target.host(), Some("example.com"));
    assert_eq!(
        serde_json::to_value(Item::Open(target)).unwrap(),
        serde_json::json!({"kind": "open", "value": {"kind": "site", "value": "https://example.com/path"}})
    );
}

#[test]
fn twelve_opening_and_guidance_routes() {
    for (text, expected) in [
        ("open YouTube", Intent::Chat),
        ("please open calculator", Intent::Chat),
        ("Can you launch Paint?", Intent::Chat),
        ("open settings", Intent::Chat),
        ("open the weather website", Intent::Chat),
        ("открой калькулятор", Intent::Chat),
        ("запусти Paint", Intent::Chat),
        ("открой сайт погоды", Intent::Chat),
        ("открой настройки", Intent::Chat),
        ("open settings in Chrome", Intent::How),
        ("открой настройки звука", Intent::How),
        ("how do I open settings?", Intent::How),
    ] {
        assert_eq!(guess_intent(text), expected, "{text}");
    }
}

#[test]
fn literal_addresses_need_no_screen_and_negations_do_not_open() {
    for text in [
        "open example.com",
        "открой https://example.com/weather",
        "don't open YouTube",
        "не открывай калькулятор",
    ] {
        assert_eq!(guess_intent(text), Intent::Chat, "{text}");
    }
}

#[test]
fn open_tags_are_dropped_unless_the_person_asked_to_open() {
    // A page on screen told the model to open something: the person never asked.
    let items = parse_with("Opening the page.<open https://evil.example.com>", None);
    assert!(!items.iter().any(|item| matches!(item, Item::Open(_))));
    assert!(
        items
            .iter()
            .any(|item| matches!(item, Item::Say(say) if say.text == "Opening the page."))
    );
    use mm_core::api::intent::wants_open;
    for asked in [
        "Open YouTube",
        "please launch the calculator",
        "Mate, start Paint",
        "Открой калькулятор",
        "запусти, пожалуйста, блокнот",
        "можешь открыть ютуб?",
    ] {
        assert!(wants_open(asked), "{asked}");
    }
    for not_asked in [
        "Is this safe?",
        "Where is the Save button?",
        "Это безопасно?",
        "What does this message say?",
        "opener", // not the word itself
        "Don't open YouTube",
        "please do not launch anything",
        "Не открывай ютуб",
        "I don't really want to open the calculator",
        "я не хочу, чтобы ты открыл ютуб",
    ] {
        assert!(!wants_open(not_asked), "{not_asked}");
    }
}

#[test]
fn an_open_tag_must_name_what_the_person_asked_for() {
    let opened = |request: &str, tag: &str| {
        parse_with(&format!("Opening.<open {tag}>"), Some(request))
            .iter()
            .any(|item| matches!(item, Item::Open(_)))
    };
    assert!(opened("Open the calculator", "app:calculator"));
    assert!(opened("Открой калькулятор", "app:calculator"));
    assert!(opened("запусти блокнот", "app:notepad"));
    assert!(opened("open youtube", "https://www.youtube.com"));
    assert!(opened("открой ютуб", "https://www.youtube.com"));
    assert!(opened("открой госуслуги", "https://www.gosuslugi.ru"));
    assert!(opened("open wikipedia.org", "https://www.wikipedia.org"));
    // The model (or a page it read) picked something else: dropped.
    assert!(!opened("Open the calculator", "app:store"));
    assert!(!opened(
        "open youtube",
        "https://bank-lookalike.example.com"
    ));
    assert!(!opened("открой ютуб", "https://www.google.com"));
    // Imitation hosts carrying the name in another label, and deep links nobody asked for.
    assert!(!opened("open youtube", "https://youtube.com.evil.example"));
    assert!(!opened("open youtube", "https://www.youtube.evil.example"));
    assert!(!opened(
        "open youtube",
        "https://www.youtube.com/redirect?q=evil"
    ));
    assert!(opened(
        "open https://www.youtube.com/feed",
        "https://www.youtube.com/feed"
    ));
    assert!(opened("open the bbc", "https://www.bbc.co.uk"));
    assert!(opened("open youtube.com", "https://www.youtube.com/"));
}
