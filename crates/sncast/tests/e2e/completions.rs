use crate::helpers::runner::runner;
use clap::ValueEnum;
use clap_complete::Shell;
use indoc::formatdoc;
use shared::test_utils::output_assert::assert_stderr_contains;

#[test]
fn test_happy_case() {
    for variant in Shell::value_variants() {
        let shell = variant.to_string();
        let args = vec!["completion", shell.as_str()];

        let snapbox = runner(&args);

        snapbox.assert().success();
    }
}

#[test]
fn test_generate_completions_unsupported_shell() {
    // SAFETY: Tests run in parallel and share the same environment variables.
    // However, this modification is applies only to this one test.
    unsafe {
        std::env::set_var("SHELL", "/bin/unsupported");
    }
    let args = vec!["completion"];

    let snapbox = runner(&args);

    let output = snapbox.assert().failure();

    assert_stderr_contains(
        output,
        formatdoc!(
            r"
            Error: Unsupported shell
            "
        ),
    );
}
