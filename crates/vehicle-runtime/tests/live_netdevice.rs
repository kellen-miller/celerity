use vehicle_runtime::{LiveNetdeviceError, verify_actuator_netdevice, verify_powertrain_netdevice};

fn details(ctrlmode: &str, bitrate: u64, restart_ms: u64) -> String {
    serde_json::json!([{
        "ifname": "can-haltech",
        "operstate": "UP",
        "linkinfo": {
            "info_kind": "can",
            "info_data": {
                "bittiming": {"bitrate": bitrate},
                "ctrlmode": [ctrlmode],
                "restart_ms": restart_ms,
            },
        },
    }])
    .to_string()
}

#[test]
fn exact_live_powertrain_attributes_are_required() {
    assert_eq!(
        verify_powertrain_netdevice(
            "can-haltech",
            "can-duct",
            &details("LISTEN-ONLY", 1_000_000, 0),
        ),
        Ok(())
    );
    assert_eq!(
        verify_powertrain_netdevice(
            "can-haltech",
            "can-duct",
            &details("LISTEN-ONLY", 500_000, 0),
        ),
        Err(LiveNetdeviceError::WrongBitrate)
    );
    assert_eq!(
        verify_powertrain_netdevice(
            "can-haltech",
            "can-duct",
            &details("LOOPBACK", 1_000_000, 0)
        ),
        Err(LiveNetdeviceError::NotListenOnly)
    );
    assert_eq!(
        verify_powertrain_netdevice(
            "can-haltech",
            "can-duct",
            &details("LISTEN-ONLY", 1_000_000, 100),
        ),
        Err(LiveNetdeviceError::AutomaticRestartEnabled)
    );
}

#[test]
fn interface_ownership_cannot_alias() {
    assert_eq!(
        verify_powertrain_netdevice(
            "can-haltech",
            "can-haltech",
            &details("LISTEN-ONLY", 1_000_000, 0),
        ),
        Err(LiveNetdeviceError::SameInterface)
    );
}

#[test]
fn actuator_requires_qualified_can_fd_without_automatic_restart() {
    let details = serde_json::json!([{
        "ifname": "can-duct",
        "operstate": "UP",
        "linkinfo": {
            "info_kind": "can",
            "info_data": {
                "bittiming": {"bitrate": 500_000},
                "data_bittiming": {"bitrate": 2_000_000},
                "ctrlmode": ["FD"],
                "restart_ms": 0,
                "state": "ERROR-ACTIVE",
            },
        },
    }])
    .to_string();
    assert_eq!(
        verify_actuator_netdevice("can-duct", "can-haltech", &details),
        Ok(())
    );

    let bus_off = details.replace("ERROR-ACTIVE", "BUS-OFF");
    assert_eq!(
        verify_actuator_netdevice("can-duct", "can-haltech", &bus_off),
        Err(LiveNetdeviceError::BusNotQualified)
    );
    let wrong_data_rate = details.replace("2000000", "1000000");
    assert_eq!(
        verify_actuator_netdevice("can-duct", "can-haltech", &wrong_data_rate),
        Err(LiveNetdeviceError::WrongDataBitrate)
    );
}
