use mm_core::privacy::{password_like, password_toggle, password_words};

#[test]
fn powershell_word_fixtures() {
    let must_match = [
        "Password",
        "Parol",
        "Пароль",
        "PIN",
        "Show password",
        "Hide password",
        "passwd",
        "passcode",
        "passphrase",
        "pwd",
        "one-time code",
        "OTP",
        "verification code",
        "пин-код",
        "kennwort",
        "contraseña",
        "mot de passe",
        "senha",
        "şifre",
        "sifre",
        "hasło",
        "heslo",
        "wachtwoord",
        "lösenord",
        "salasana",
        "jelszó",
        "parola",
        "κωδικός",
        "密码",
        "密碼",
        "パスワード",
        "비밀번호",
        "كلمة المرور",
        "סיסמה",
        "mật khẩu",
        "kata sandi",
        "รหัสผ่าน",
        "CVV",
        "CVC",
        "security code",
        "Parolingiz",
        // The second PowerShell mustMatch list exercises token boundaries.
        "PIN码",
        "txt_pin",
        "pinBox",
        "otp_input",
        "ПИН",
        "пин",
    ];
    for label in must_match {
        assert!(password_words(label), "expected match: {label}");
    }
    for label in [
        "Pinned",
        "Passport",
        "Spinner",
        "Bypass",
        "Login",
        "Search site",
        "пинать",
        "Opinion",
        "Shopping",
    ] {
        assert!(!password_words(label), "unexpected match: {label}");
    }
}

#[test]
fn powershell_toggle_fixtures() {
    for label in [
        "Show password",
        "Hide password",
        "Parolni ko'rsatish",
        "Показать пароль",
    ] {
        assert!(password_toggle(label), "expected toggle match: {label}");
    }
    for label in ["Pin tab", "Pin to Quick access", "PIN", "OTP"] {
        assert!(!password_toggle(label), "unexpected toggle match: {label}");
    }
}

#[test]
fn powershell_control_fixtures() {
    let cases = [
        (true, "Edit", "", "password", "", ""),
        (true, "Edit", "Parol", "", "", ""),
        (false, "Edit", "Search site", "", "", ""),
        (true, "Custom", "", "togglePassword", "", ""),
        (true, "Image", "Show password", "", "", ""),
        (true, "Hyperlink", "Forgot password?", "", "", ""),
        (true, "Button", "", "RevealButton", "", ""),
        (true, "Edit", "", "", "", "PasswordBox"),
        (false, "Button", "Pin tab", "", "", ""),
        (false, "Text", "Password", "", "", ""),
        (false, "Window", "Change password", "", "", ""),
    ];
    for (expected, kind, name, id, help, class) in cases {
        assert_eq!(
            password_like(kind, name, id, help, class),
            expected,
            "{kind} | {name} | {id} | {help} | {class}",
        );
    }
}

#[test]
fn control_branch_precedence_and_case() {
    for kind in ["Text", "Document", "Window", "Pane", "text"] {
        assert!(!password_like(kind, "Password", "RevealButton", "PIN", ""));
        assert!(password_like(kind, "", "", "", "passwordbox"));
    }
    for label in ["PIN", "txt_pin", "otp_input", "CVV", "CVC", "пин"] {
        assert!(password_like("Edit", "", "", label, ""));
        assert!(!password_like("Button", label, label, "", ""));
    }
    for kind in ["Image", "Hyperlink", "Button", "", "Unknown"] {
        assert!(password_like(kind, "Show password", "", "", ""));
        assert!(password_like(kind, "", "togglePassword", "", ""));
        assert!(password_like(kind, "", "revealbutton", "", ""));
        assert!(!password_like(kind, "", "", "password", ""));
    }
    // Custom controls can be unmasked PIN/OTP fields: fail closed.
    assert!(password_like(
        "Custom",
        "Pin",
        "pin-button",
        "Pin this message",
        ""
    ));
    assert!(password_like("Custom", "", "", "password", ""));
    assert!(password_like("Custom", "", "revealbutton", "", ""));
    assert!(password_like("eDiT", "", "PIN", "", ""));
    assert!(!password_like("Edit", "", "RevealButton", "", ""));
    assert!(!password_like("Edit", "", "", "", "MyPasswordBox"));
    assert!(!password_like("Button", "", "prefixRevealButton", "", ""));
}

#[test]
fn short_tokens_keep_case_sensitive_ascii_neighbours() {
    for token in ["pwd", "pin", "otp", "cvv", "cvc"] {
        for label in [
            format!("_{token}_"),
            format!("A{token}B"),
            format!("码{token}1"),
        ] {
            assert!(password_words(&label), "expected match: {label}");
        }
        for label in [format!("a{token}"), format!("{token}b")] {
            assert!(!password_words(&label), "unexpected match: {label}");
        }
    }
    for label in ["пинать", "апин", "ПИНАТЬ", "Ёпин", "пинЁ"] {
        assert!(!password_words(label), "unexpected match: {label}");
    }
    for label in ["_ПиН_", "码пин", "пин1"] {
        assert!(password_words(label), "expected match: {label}");
    }
}

#[test]
fn long_words_keep_boundaries_and_whitespace() {
    for label in [
        "Parolingiz",
        "_parola",
        "verification\tcode",
        "mot\nde passe",
        "one time-code",
    ] {
        assert!(password_toggle(label), "expected match: {label}");
    }
    for label in ["xparol", "_parol", "码parol", "one_time_code"] {
        assert!(!password_toggle(label), "unexpected match: {label}");
    }
}

#[test]
fn matching_is_independent_of_turkish_case_rules() {
    for label in [
        "İ/ı: PIN",
        "SIFRE",
        "ŞIFRE",
        "SECURITY CODE",
        "VERIFICATION CODE",
    ] {
        assert!(password_words(label), "expected invariant match: {label}");
    }
    for label in ["sıfre", "şıfre", "PıN", "PİN"] {
        assert!(!password_words(label), "unexpected Turkish fold: {label}");
    }
}
#[test]
fn click_safety_whole_words_and_amounts() {
    for name in [
        "Pay this electricity bill for me",
        "pagar",
        "pague",
        "pago",
        "comprar",
        "compra",
        "compre",
        "sil",
        "SİL",
        "O'tkazma",
        "O‘tkazish",
        "Подписаться на канал",
        "Subscribe",
        "Видалити",
        "Оплатити",
        "Купити",
        "Переказати",
        "Отправить 500 ₽",
        "Отправить 500",
        "Continue – $9.99",
        "€5",
        "5 €",
        "£ 20",
        "20£",
        "Continue 500 ₽",
        "500 so'm",
        "so‘m 500",
        "500 сум",
        "500 руб.",
        "500 тенге",
        "500 ₹",
        "￥500",
        "Proceed 9,99€",
    ] {
        assert!(mm_core::privacy::forbidden(name, "Button"), "{name}");
    }
    for name in [
        "Next page",
        "Page setup",
        "Silent",
        "Compress",
        "O'tkazib yuborish",
        "O‘tkazib yuborish",
        "page",
        "silence",
        "500 results",
        "Currency $",
        "Send feedback",
    ] {
        assert!(!mm_core::privacy::forbidden(name, "Button"), "{name}");
    }
}
#[test]
fn broad_payment_delete_and_transfer_stems_keep_only_exact_harmless_exceptions() {
    for text in [
        "Paga ora",
        "Conferma pagamento",
        "silmek",
        "silinsin",
        "o'tkazmani",
        "O'tkazing",
        "comprare",
        "page pagamento",
        "silent silmek",
        "compression compra",
    ] {
        assert!(mm_core::privacy::forbidden(text, "Button"), "{text}");
    }
    for text in [
        "Next page",
        "pages",
        "paging",
        "Silent",
        "silence",
        "silently",
        "Compress",
        "compression",
        "O'tkazib yuborish",
    ] {
        assert!(!mm_core::privacy::forbidden(text, "Button"), "{text}");
    }
}
