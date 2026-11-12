//! Local account commands, independent of the running app.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum AntigravityAction {
    Login,
    Logout,
    Status,
}

pub(super) fn run(action: AntigravityAction) -> u8 {
    let result = match action {
        AntigravityAction::Login if !crate::labs::antigravity_summaries() => {
            Err(color_eyre::eyre::eyre!(
                "{}",
                crate::desktop::i18n::tr("Turn on Antigravity summaries in Labs")
            ))
        }
        AntigravityAction::Login => {
            let show = |url: &str| {
                eprintln!(
                    "{}\n{url}",
                    crate::desktop::i18n::tr("Open this address to sign in:")
                )
            };
            crate::antigravity::login(&std::sync::atomic::AtomicBool::new(false), Some(&show))
        }
        AntigravityAction::Logout => crate::antigravity::logout(),
        AntigravityAction::Status => {
            if let Err(error) = crate::antigravity::client_secret() {
                eprintln!("hex: {error}");
            }
            println!(
                "{}",
                crate::antigravity::email()
                    .unwrap_or_else(|| crate::desktop::i18n::tr("Sign in to Antigravity").into())
            );
            Ok(())
        }
    };
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("hex: {e}");
            1
        }
    }
}
