mod protocol;

use std::io::{self, Write};

use protocol::{
    ACK_FRESHNESS_MS, Action, COMMAND_LEASE_MS, FeatureAuthority, RUNTIME_LEASE_MS,
    RadiatorFraction, Simulation, reduce,
};

const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

fn main() -> io::Result<()> {
    let mut state = Simulation::new();
    let stdin = io::stdin();

    loop {
        render(&state);
        print!("\n{BOLD}action>{RESET} ");
        io::stdout().flush()?;

        let mut input = String::new();
        if stdin.read_line(&mut input)? == 0 {
            break;
        }

        let input = input.trim();
        if input == "q" {
            break;
        }

        let action = match input {
            "i" => Some(Action::CompleteSelfTest),
            "d" => Some(Action::Discover),
            "g" => Some(Action::GrantOrRenewRuntimeLease),
            "3" => Some(Action::Command(
                RadiatorFraction::from_basis_points(3_000).expect("constant is valid"),
            )),
            "7" => Some(Action::Command(
                RadiatorFraction::from_basis_points(7_000).expect("constant is valid"),
            )),
            "t" => Some(Action::AdvanceTime(50)),
            "T" => Some(Action::AdvanceTime(250)),
            "l" => Some(Action::ToggleLink),
            "s" => Some(Action::SendStaleSequence),
            "e" => Some(Action::SendWrongEpoch),
            "u" => Some(Action::ConfigureNextGeneration),
            "f" => Some(Action::InjectHardFault),
            "b" => Some(Action::PowerCycle),
            "x" => Some(Action::ControlledShutdown),
            _ => None,
        };

        if let Some(action) = action {
            state = reduce(state, action);
        } else {
            state.last_action = "Unknown action";
            state.exchange = vec![format!("INPUT Unknown action {input:?}")];
        }
    }

    println!("\nPrototype exited; all state was in memory.");
    Ok(())
}

fn render(state: &Simulation) {
    print!("\x1b[2J\x1b[H");
    println!("{BOLD}CELERITY CONTROLLER CONFORMANCE — THROWAWAY PROTOTYPE{RESET}");
    println!(
        "{DIM}Accelerated demo timing: runtime={}ms command={}ms ack={}ms{RESET}",
        RUNTIME_LEASE_MS, COMMAND_LEASE_MS, ACK_FRESHNESS_MS
    );
    println!();
    println!("{BOLD}Simulation{RESET}");
    field("monotonic time", format!("{}ms", state.now_ms));
    field("CAN FD link", if state.link_up { "UP" } else { "DOWN" });
    field("last action", state.last_action);
    println!();
    println!("{BOLD}Vehicle Compute Node view{RESET}");
    field("observed boot", option(state.compute.observed_boot_session));
    field(
        "observed lifecycle",
        debug_option(state.compute.observed_lifecycle),
    );
    field("observed mode", debug_option(state.compute.observed_mode));
    field(
        "observed fault",
        debug_option(state.compute.observed_fault_latch),
    );
    field(
        "observed capability",
        option(state.compute.observed_capability_generation),
    );
    field(
        "observed config",
        option(state.compute.observed_config_generation),
    );
    field(
        "feature authority",
        format!("{:?}", state.compute.feature_authority),
    );
    field("runtime epoch", state.compute.epoch);
    field("next command sequence", state.compute.next_command_sequence);
    field(
        "last command ACK",
        state
            .compute
            .last_command_ack_at_ms
            .map_or_else(|| "never".to_owned(), |time| format!("{time}ms")),
    );
    field(
        "last heartbeat",
        state
            .compute
            .last_heartbeat_at_ms
            .map_or_else(|| "never".to_owned(), |time| format!("{time}ms")),
    );
    field(
        "last ACK result",
        debug_option(state.compute.last_ack_result),
    );
    println!();
    println!("{BOLD}Controller truth{RESET}");
    field("identity", "provisioned node 0x002a");
    field("boot session", state.controller.boot_session);
    field("lifecycle", format!("{:?}", state.controller.lifecycle));
    field("actuation mode", format!("{:?}", state.controller.mode));
    field("fault latch", format!("{:?}", state.controller.fault_latch));
    field("config generation", state.controller.config_generation);
    field("accepted setpoint", state.controller.accepted_fraction);
    field(
        "runtime lease",
        state.controller.runtime_lease.as_ref().map_or_else(
            || "none".to_owned(),
            |lease| {
                format!(
                    "epoch={} renewal={} expires={}ms",
                    lease.epoch, lease.renewal_sequence, lease.expires_at_ms
                )
            },
        ),
    );
    field(
        "command lease",
        state
            .controller
            .command_expires_at_ms
            .map_or_else(|| "none".to_owned(), |time| format!("expires={time}ms")),
    );
    field(
        "last accepted sequence",
        option(state.controller.last_command_sequence),
    );
    println!();
    println!("{BOLD}Last exchange{RESET}");
    for event in &state.exchange {
        println!("  {event}");
    }
    println!();
    println!("{BOLD}Actions{RESET}");
    println!(
        "  {BOLD}i{RESET} {DIM}self-test{RESET}  {BOLD}d{RESET} {DIM}discover{RESET}  {BOLD}g{RESET} {DIM}grant/renew runtime{RESET}  {BOLD}3{RESET}/{BOLD}7{RESET} {DIM}command 30%/70%{RESET}"
    );
    println!(
        "  {BOLD}t{RESET} {DIM}+50ms{RESET}  {BOLD}T{RESET} {DIM}+250ms{RESET}  {BOLD}l{RESET} {DIM}toggle link{RESET}  {BOLD}s{RESET} {DIM}stale sequence{RESET}  {BOLD}e{RESET} {DIM}wrong epoch{RESET}"
    );
    println!(
        "  {BOLD}u{RESET} {DIM}new config{RESET}  {BOLD}f{RESET} {DIM}hard fault{RESET}  {BOLD}b{RESET} {DIM}power cycle{RESET}  {BOLD}x{RESET} {DIM}controlled fallback{RESET}  {BOLD}q{RESET} {DIM}quit{RESET}"
    );

    if state.compute.feature_authority == FeatureAuthority::Active
        && state.controller.mode != protocol::ActuationMode::Commanded
    {
        println!(
            "\n{BOLD}NOTE{RESET} host observation is stale: it still says Active while the controller is already in Fallback"
        );
    }
}

fn field(label: &str, value: impl std::fmt::Display) {
    println!("  {BOLD}{label:<23}{RESET} {value}");
}

fn option<T: std::fmt::Display>(value: Option<T>) -> String {
    value.map_or_else(|| "unknown".to_owned(), |value| value.to_string())
}

fn debug_option<T: std::fmt::Debug>(value: Option<T>) -> String {
    value.map_or_else(|| "unknown".to_owned(), |value| format!("{value:?}"))
}
