//! Mail body rendering pipeline tests (structure, not pixels).
//!
//! Covers security defaults, Microsoft-style plain fallbacks, and HTML fidelity
//! through `build_mail_body` / `parse_raw_message`.

use mailbox_core::message_parse::parse_raw_message;
use mailbox_core::{build_mail_body, BodyBlock, InlineSeg, MailBody};

fn all_text(body: &MailBody) -> String {
    let mut out = String::new();
    for block in &body.blocks {
        match block {
            BodyBlock::Paragraph(spans)
            | BodyBlock::Bullet(spans)
            | BodyBlock::Quote(spans)
            | BodyBlock::Heading { spans, .. } => {
                for s in spans {
                    match s {
                        InlineSeg::Text(t) => out.push_str(t),
                        InlineSeg::Link { text, .. } => out.push_str(text),
                    }
                    out.push(' ');
                }
            }
            BodyBlock::Image { alt, .. } | BodyBlock::ImageBlocked { alt } => {
                out.push_str(alt);
                out.push(' ');
            }
            BodyBlock::Rule => {}
        }
    }
    out
}

fn links(body: &MailBody) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for block in &body.blocks {
        let spans = match block {
            BodyBlock::Paragraph(s)
            | BodyBlock::Bullet(s)
            | BodyBlock::Quote(s)
            | BodyBlock::Heading { spans: s, .. } => s,
            _ => continue,
        };
        for seg in spans {
            if let InlineSeg::Link { text, url } = seg {
                out.push((text.clone(), url.clone()));
            }
        }
    }
    out
}

fn has_link(body: &MailBody, label_substr: &str, url_substr: &str) -> bool {
    links(body).iter().any(|(t, u)| {
        t.contains(label_substr) && u.contains(url_substr)
    })
}

fn link_urls(body: &MailBody) -> Vec<String> {
    links(body).into_iter().map(|(_, u)| u).collect()
}

#[test]
fn microsoft_plain_fixture_no_brackets_and_labeled_links() {
    let plain = include_str!("fixtures/mail/microsoft_ignite_plain.txt");
    let body = build_mail_body(plain, "", false);
    let text = all_text(&body);

    assert!(!text.contains("[["), "raw brackets leaked: {text}");
    assert!(
        text.contains("Join the software development community"),
        "{text}"
    );
    assert!(
        has_link(&body, "View as a web page", "emails.microsoft.com/v/"),
        "{:?}",
        links(&body)
    );
    assert!(
        has_link(&body, "Innovation Basecamp", "aka.ms/Ignite26"),
        "{:?}",
        links(&body)
    );
    assert!(
        has_link(&body, "Facebook", "emails.microsoft.com"),
        "{:?}",
        links(&body)
    );
    assert!(
        has_link(&body, "Unsubscribe", "account.microsoft.com"),
        "{:?}",
        links(&body)
    );
    // Orphan logo URL must not become a link or visible text.
    assert!(
        !text.contains("www.microsoft.com"),
        "orphan logo URL leaked: {text}"
    );
    assert!(
        link_urls(&body)
            .iter()
            .all(|u| u.starts_with("http://") || u.starts_with("https://") || u.starts_with("mailto:")),
        "{:?}",
        link_urls(&body)
    );
}

#[test]
fn marketing_html_preserves_structure_and_blocks_images() {
    let html = include_str!("fixtures/mail/marketing_html_min.html");
    let blocked = build_mail_body("", html, false);
    let text = all_text(&blocked);

    assert!(text.contains("Microsoft Ignite is your stage too"), "{text}");
    assert!(text.contains("Bring your perspective"), "{text}");
    assert!(!text.contains(".hidden"), "CSS leaked: {text}");
    assert!(!text.contains("display:none"), "CSS leaked: {text}");

    assert!(blocked.has_remote_images);
    assert!(
        blocked
            .blocks
            .iter()
            .any(|b| matches!(b, BodyBlock::ImageBlocked { alt } if alt.contains("Microsoft logo"))),
        "{:?}",
        blocked.blocks
    );
    assert!(
        blocked
            .blocks
            .iter()
            .any(|b| matches!(b, BodyBlock::ImageBlocked { .. })),
        "expected ImageBlocked"
    );
    assert!(
        !blocked
            .blocks
            .iter()
            .any(|b| matches!(b, BodyBlock::Image { .. })),
        "images must stay blocked by default"
    );

    assert!(
        has_link(
            &blocked,
            "Microsoft for Startups Innovation Basecamp",
            "aka.ms/Ignite26"
        ),
        "{:?}",
        links(&blocked)
    );
    assert!(
        has_link(&blocked, "Contact support", "mailto:support@example.com"),
        "{:?}",
        links(&blocked)
    );
    assert!(
        blocked
            .blocks
            .iter()
            .any(|b| matches!(b, BodyBlock::Heading { level: 1, .. })),
        "expected h1"
    );
    assert!(
        blocked
            .blocks
            .iter()
            .any(|b| matches!(b, BodyBlock::Bullet(_))),
        "expected list items"
    );
    assert!(
        blocked
            .blocks
            .iter()
            .any(|b| matches!(b, BodyBlock::Quote(_))),
        "expected blockquote"
    );
    assert!(
        blocked
            .blocks
            .iter()
            .any(|b| matches!(b, BodyBlock::Rule)),
        "expected hr"
    );

    let loaded = build_mail_body("", html, true);
    assert!(loaded.has_remote_images);
    assert!(
        loaded
            .blocks
            .iter()
            .any(|b| matches!(b, BodyBlock::Image { url, .. } if url.contains("ms-logo.png"))),
        "{:?}",
        loaded.blocks
    );
    // 1×1 /pixel.gif is a tracking threat — stays blocked even when loading is allowed.
    assert!(
        loaded.has_image_threats(),
        "expected tracking pixel threat: {:?}",
        loaded.image_threats
    );
    assert!(
        loaded
            .blocks
            .iter()
            .any(|b| matches!(b, BodyBlock::ImageBlocked { alt } if alt.contains("tracker"))),
        "tracking pixel must stay ImageBlocked: {:?}",
        loaded.blocks
    );
}

#[test]
fn hostile_html_strips_scripts_and_rejects_dangerous_urls() {
    let html = include_str!("fixtures/mail/hostile.html");
    let body = build_mail_body("", html, false);
    let text = all_text(&body);
    let urls = link_urls(&body);

    assert!(text.contains("Hello"), "{text}");
    assert!(text.contains("friend"), "{text}");
    assert!(text.contains("Visible after sanitize"), "{text}");
    assert!(!text.contains("document.cookie"), "script leaked: {text}");
    assert!(!text.contains("alert(1)"), "script/handler leaked: {text}");

    assert!(
        has_link(&body, "friend", "https://safe.example/path"),
        "{:?}",
        links(&body)
    );
    assert!(
        urls.iter().all(|u| {
            u.starts_with("https://") || u.starts_with("http://") || u.starts_with("mailto:")
        }),
        "unsafe scheme survived: {urls:?}"
    );
    assert!(
        !urls.iter().any(|u| {
            let l = u.to_ascii_lowercase();
            l.starts_with("javascript:") || l.starts_with("data:") || l.starts_with("vbscript:")
        }),
        "{urls:?}"
    );
    // javascript: img must not become an Image block.
    assert!(
        !body
            .blocks
            .iter()
            .any(|b| matches!(b, BodyBlock::Image { .. } | BodyBlock::ImageBlocked { .. })),
        "{:?}",
        body.blocks
    );
}

#[test]
fn html_preferred_over_microsoft_plain_when_both_present() {
    let plain = include_str!("fixtures/mail/microsoft_ignite_plain.txt");
    let html = include_str!("fixtures/mail/marketing_html_min.html");
    let body = build_mail_body(plain, html, false);
    let text = all_text(&body);

    // HTML path wins — marketing heading, not only plain fallback.
    assert!(text.contains("Microsoft Ignite is your stage too"), "{text}");
    assert!(body.has_remote_images);
    assert!(
        has_link(&body, "Innovation Basecamp", "aka.ms/Ignite26"),
        "{:?}",
        links(&body)
    );
    // Plain-only orphan brackets must not appear when HTML is used.
    assert!(!text.contains("[["), "{text}");
}

#[test]
fn multipart_eml_stores_html_and_prefers_html_links() {
    let raw = include_bytes!("fixtures/mail/multipart_alt.eml");
    let msg = parse_raw_message("acct", "INBOX", 42, true, raw).expect("parse eml");

    assert!(
        !msg.body_html.is_empty(),
        "expected HTML part stored"
    );
    assert!(
        msg.body_html.contains("Join Ignite HTML") || msg.body_html.contains("aka.ms/from-html"),
        "{}",
        msg.body_html
    );

    let body = build_mail_body(&msg.body_text, &msg.body_html, false);
    let text = all_text(&body);
    assert!(text.contains("Join Ignite HTML") || text.contains("Explore sessions"), "{text}");
    assert!(
        has_link(&body, "Explore sessions", "aka.ms/from-html"),
        "HTML link should win over plain: {:?}",
        links(&body)
    );
    assert!(body.has_remote_images);
    assert!(
        body.blocks
            .iter()
            .any(|b| matches!(b, BodyBlock::ImageBlocked { alt } if alt == "Hero")),
        "{:?}",
        body.blocks
    );
}

#[test]
fn plain_only_multipart_fallback_still_linkifies() {
    // Simulate old cache: HTML never stored, only Microsoft-style plain.
    let plain = "View as a web page <[[https://emails.microsoft.com/v/x]]>\n\nHello Ignite";
    let body = build_mail_body(plain, "", false);
    assert!(all_text(&body).contains("Hello Ignite"));
    assert!(has_link(&body, "View as a web page", "https://emails.microsoft.com/v/x"));
    assert!(!body.has_remote_images);
}

#[test]
fn linkedin_plain_linkifies_without_dumping_query_strings() {
    let plain = include_str!("fixtures/mail/linkedin_job_alert_plain.txt");
    let body = build_mail_body(plain, "", false);
    let text = all_text(&body);

    assert!(text.contains("Senior Drupal Developer"), "{text}");
    assert!(text.contains("General Dynamics Information Technology"), "{text}");
    assert!(text.contains("This company is actively hiring"), "{text}");
    // Huge tracking / otpToken blobs must not appear as visible text.
    assert!(!text.contains("otpToken"), "otpToken leaked into text: {text}");
    assert!(!text.contains("ZDQ0YjQwYjkyNTk0"), "token blob leaked: {text}");
    assert!(
        has_link(&body, "View job", "linkedin.com/comm/jobs/view/4493351964"),
        "{:?}",
        links(&body)
    );
    assert!(
        has_link(&body, "See all jobs on LinkedIn", "linkedin.com/comm/jobs/search-results"),
        "{:?}",
        links(&body)
    );
    assert!(
        has_link(&body, "Unsubscribe", "job-alert-email-unsubscribe"),
        "{:?}",
        links(&body)
    );
    // Link labels stay short (not the full URL).
    for (label, url) in links(&body) {
        assert!(
            label.len() < 80,
            "label too long ({}): {label} url_len={}",
            label.len(),
            url.len()
        );
        assert!(url.starts_with("https://"), "{url}");
    }
    assert!(
        body.blocks.iter().any(|b| matches!(b, BodyBlock::Rule)),
        "expected dashed rule lines"
    );
}
