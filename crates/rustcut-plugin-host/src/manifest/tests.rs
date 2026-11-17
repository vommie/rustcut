use super::*;

const MINIMAL: &str = r#"
id = "auto-folder"
name = "Auto-Folder"
version = "1.2.3"
api = "1.0"
"#;

#[test]
fn a_minimal_manifest_is_valid() {
    let manifest = Manifest::parse(MINIMAL).unwrap();
    assert_eq!(manifest.id, "auto-folder");
    assert_eq!(manifest.wasm, PathBuf::from("plugin.wasm"));
    assert!(manifest.api_compatible());
}

#[test]
fn ids_follow_the_rules() {
    for good in ["ab", "auto-folder", "x1-y2"] {
        assert!(valid_id(good), "{good}");
    }
    for bad in ["a", "Auto", "1abc", "a--b", "ab-", "a_b", "äb", &"a".repeat(41)] {
        assert!(!valid_id(bad), "{bad}");
    }
}

#[test]
fn api_versions_decide_compatibility() {
    let with = |api: &str| Manifest::parse(&MINIMAL.replace("api = \"1.0\"", &format!("api = \"{api}\""))).unwrap();
    assert!(with("1.0.0").api_compatible());
    assert!(with("1.1").api_compatible());
    assert!(with("1.2").api_compatible());
    assert!(!with("1.3").api_compatible());
    assert!(!with("2.0").api_compatible());
    assert!(!with("0.9").api_compatible());
    assert!(Manifest::parse(&MINIMAL.replace("api = \"1.0\"", "api = \"one\"")).is_err());
}

#[test]
fn unknown_keys_and_bad_settings_are_refused() {
    assert!(Manifest::parse(&format!("{MINIMAL}\nfoo = 1")).is_err());
    let twice = format!(
        "{MINIMAL}\n[[cards]]\ntitle = \"A\"\n[[cards.fields]]\nkey = \"x\"\nlabel = \"X\"\nkind = \"switch\"\n\
         [[cards.fields]]\nkey = \"x\"\nlabel = \"Y\"\nkind = \"tag\"\n"
    );
    let err = Manifest::parse(&twice).unwrap_err().to_string();
    assert!(err.contains("used twice"), "{err}");
    let choice = format!(
        "{MINIMAL}\n[[cards]]\ntitle = \"A\"\n[[cards.fields]]\nkey = \"m\"\nlabel = \"M\"\nkind = \"choice\"\n\
         default = \"z\"\noptions = [{{ value = \"a\", label = \"A\" }}]\n"
    );
    assert!(
        Manifest::parse(&choice)
            .unwrap_err()
            .to_string()
            .contains("not an option")
    );
    let cond = format!(
        "{MINIMAL}\n[[cards]]\ntitle = \"A\"\n[[cards.fields]]\nkey = \"m\"\nlabel = \"M\"\nkind = \"switch\"\n\
         enabled_if = {{ key = \"nope\", equals = true }}\n"
    );
    assert!(Manifest::parse(&cond).unwrap_err().to_string().contains("unknown key"));
}

#[test]
fn the_manifest_survives_a_round_trip() {
    let manifest = Manifest::parse(&format!(
        "{MINIMAL}\n[[cards]]\ntitle = \"Rules\"\n[[cards.fields]]\nkey = \"rules\"\nlabel = \"Rules\"\nkind = \"table\"\n\
         [[cards.fields.columns]]\nkey = \"source\"\nlabel = \"Source\"\nkind = \"path\"\n"
    ))
    .unwrap();
    assert_eq!(Manifest::parse(&manifest.to_toml().unwrap()).unwrap(), manifest);
}

const WITH_KEYS: &str = "\n[[cards]]\ntitle = \"Keys\"\n[[cards.fields]]\nkey = \"next_key\"\nlabel = \"Next\"\nkind = \"key\"\n\
    default = \"Control+PageDown\"\n[[cards.fields]]\nkey = \"buttons\"\nlabel = \"Buttons\"\nkind = \"switch\"\n";

#[test]
fn commands_name_their_key_and_button_fields() {
    let good = format!(
        "{MINIMAL}{WITH_KEYS}\n[[commands]]\nid = \"next-file\"\ntitle = \"Next File\"\nicon = \"player-track-next\"\n\
         key = \"next_key\"\nmenu = true\nbutton = \"buttons\"\ndiscards_sections = true\n"
    );
    let manifest = Manifest::parse(&good).unwrap();
    let command = &manifest.commands[0];
    assert!(command.menu && command.discards_sections);
    assert_eq!(command.button, "buttons");
    assert!(Manifest::parse(&manifest.to_toml().unwrap()).is_ok());

    for (from, to) in [
        ("key = \"next_key\"\nmenu", "key = \"buttons\"\nmenu"),
        ("button = \"buttons\"", "button = \"next_key\""),
        ("id = \"next-file\"", "id = \"Next\""),
        ("title = \"Next File\"", "title = \"\""),
        ("icon = \"player-track-next\"", "icon = \"icon.svg\""),
    ] {
        assert!(Manifest::parse(&good.replace(from, to)).is_err(), "{to}");
    }
    let twice = format!("{good}\n[[commands]]\nid = \"next-file\"\ntitle = \"Again\"\nkey = \"next_key\"\n");
    assert!(Manifest::parse(&twice).is_err());
}

#[test]
fn key_fields_stay_out_of_tables() {
    let table = format!(
        "{MINIMAL}\n[[cards]]\ntitle = \"T\"\n[[cards.fields]]\nkey = \"rows\"\nlabel = \"Rows\"\nkind = \"table\"\n\
         [[cards.fields.columns]]\nkey = \"k\"\nlabel = \"K\"\nkind = \"key\"\n"
    );
    assert!(Manifest::parse(&table).is_err());
}
