use jsonwebtoken::{encode, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    exp: Option<usize>,
    access: AccessLevel,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum AccessLevel {
    All,
    Repositories { repos: Vec<String> },
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        eprintln!("Usage: {} <secret> <subject> [repo1,repo2,...]", args[0]);
        eprintln!("Examples:");
        eprintln!("  {} my-secret user123", args[0]);
        eprintln!("  {} my-secret user123 alpine,nginx", args[0]);
        std::process::exit(1);
    }

    let secret = &args[1];
    let subject = &args[2];

    let access = if args.len() > 3 {
        let repos: Vec<String> = args[3].split(',').map(|s| s.to_string()).collect();
        AccessLevel::Repositories { repos }
    } else {
        AccessLevel::All
    };

    let claims = Claims {
        sub: subject.to_string(),
        exp: None,
        access,
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .expect("Failed to encode token");

    println!("{}", token);
}
