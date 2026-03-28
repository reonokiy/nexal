use clap::Parser;
use nexal_arg0::Arg0DispatchPaths;
use nexal_arg0::arg0_dispatch_or_else;
use nexal_tui::AppExitInfo;
use nexal_tui::Cli;
use nexal_tui::ExitReason;
use nexal_tui::run_main;
use nexal_tui::update_action::UpdateAction;
use nexal_utils_cli::CliConfigOverrides;

#[derive(Parser, Debug)]
struct TopCli {
    #[clap(flatten)]
    config_overrides: CliConfigOverrides,

    #[clap(flatten)]
    inner: Cli,
}

fn into_app_server_cli(cli: Cli) -> nexal_tui_app_server::Cli {
    nexal_tui_app_server::Cli {
        prompt: cli.prompt,
        images: cli.images,
        resume_picker: cli.resume_picker,
        resume_last: cli.resume_last,
        resume_session_id: cli.resume_session_id,
        resume_show_all: cli.resume_show_all,
        fork_picker: cli.fork_picker,
        fork_last: cli.fork_last,
        fork_session_id: cli.fork_session_id,
        fork_show_all: cli.fork_show_all,
        model: cli.model,
        oss: cli.oss,
        oss_provider: cli.oss_provider,
        config_profile: cli.config_profile,
        sandbox_mode: cli.sandbox_mode,
        approval_policy: cli.approval_policy,
        full_auto: cli.full_auto,
        dangerously_bypass_approvals_and_sandbox: cli.dangerously_bypass_approvals_and_sandbox,
        cwd: cli.cwd,
        web_search: cli.web_search,
        add_dir: cli.add_dir,
        no_alt_screen: cli.no_alt_screen,
        config_overrides: cli.config_overrides,
    }
}

fn into_legacy_update_action(
    action: nexal_tui_app_server::update_action::UpdateAction,
) -> UpdateAction {
    match action {
        nexal_tui_app_server::update_action::UpdateAction::NpmGlobalLatest => {
            UpdateAction::NpmGlobalLatest
        }
        nexal_tui_app_server::update_action::UpdateAction::BunGlobalLatest => {
            UpdateAction::BunGlobalLatest
        }
        nexal_tui_app_server::update_action::UpdateAction::BrewUpgrade => UpdateAction::BrewUpgrade,
    }
}

fn into_legacy_exit_reason(reason: nexal_tui_app_server::ExitReason) -> ExitReason {
    match reason {
        nexal_tui_app_server::ExitReason::UserRequested => ExitReason::UserRequested,
        nexal_tui_app_server::ExitReason::Fatal(message) => ExitReason::Fatal(message),
    }
}

fn into_legacy_exit_info(exit_info: nexal_tui_app_server::AppExitInfo) -> AppExitInfo {
    AppExitInfo {
        token_usage: exit_info.token_usage,
        thread_id: exit_info.thread_id,
        thread_name: exit_info.thread_name,
        update_action: exit_info.update_action.map(into_legacy_update_action),
        exit_reason: into_legacy_exit_reason(exit_info.exit_reason),
    }
}

fn main() -> anyhow::Result<()> {
    arg0_dispatch_or_else(|arg0_paths: Arg0DispatchPaths| async move {
        let top_cli = TopCli::parse();
        let mut inner = top_cli.inner;
        inner
            .config_overrides
            .raw_overrides
            .splice(0..0, top_cli.config_overrides.raw_overrides);
        let use_app_server_tui = nexal_tui::should_use_app_server_tui(&inner).await?;
        let exit_info = if use_app_server_tui {
            into_legacy_exit_info(
                nexal_tui_app_server::run_main(
                    into_app_server_cli(inner),
                    arg0_paths,
                    nexal_core::config_loader::LoaderOverrides::default(),
                    /*remote*/ None,
                    /*remote_auth_token*/ None,
                )
                .await?,
            )
        } else {
            run_main(
                inner,
                arg0_paths,
                nexal_core::config_loader::LoaderOverrides::default(),
            )
            .await?
        };
        let token_usage = exit_info.token_usage;
        if !token_usage.is_zero() {
            println!(
                "{}",
                nexal_protocol::protocol::FinalOutput::from(token_usage),
            );
        }
        Ok(())
    })
}
