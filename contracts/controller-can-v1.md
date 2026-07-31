# Celerity controller CAN FD contract v1

This is the normative, fixed production v1 contract between `celerityd` and a
statically provisioned actuator controller. Frames use standard 11-bit CAN IDs,
ISO CAN FD, bit-rate switching, and fixed legal CAN FD payload lengths. All
multi-byte integers are unsigned little-endian. Address 0 is reserved; node
addresses are 1 through 63. Unknown and reserved IDs are ignored. A known frame
with an invalid length, range, enum, or nonzero reserved byte is rejected.

The actuator bus is commissioned at 500 kbit/s nominal and 2 Mbit/s data rate,
with automatic bus-off restart disabled. ISO CAN FD supplies the link CRC. v1
has no application checksum, MAC, alias, or compatibility translator.

## Identifier and payload allocation

Node-specific IDs add the provisioned node address to the listed base.

| ID | Frame | Bytes | Fields in byte-offset order |
| --- | --- | ---: | --- |
| `0x040 + node` | FaultReport | 24 | boot session `u32`, fault sequence `u32`, fault code `u16`, severity `u8`, flags `u8`, related epoch `u64`, related command sequence `u32` |
| `0x080` | DiscoveryProbe | 8 | protocol major `u8`, minor `u8`, flags `u16`, probe sequence `u32` |
| `0x100 + node` | Command | 24 | boot session `u32`, configuration generation `u32`, epoch `u64`, command sequence `u32`, radiator split command `u16`, flags `u8`, reserved zero `u8` |
| `0x140 + node` | CommandAck | 32 | boot session `u32`, configuration generation `u32`, epoch `u64`, command sequence `u32`, accepted command `u16`, mode `u8`, fault latch `u8`, result `u8`, output state `u8`, Command Lease remaining ms `u16`, acknowledgement sequence `u32` |
| `0x180 + node` | RuntimeLease | 24 | boot session `u32`, configuration generation `u32`, epoch `u64`, renewal sequence `u32`, validity ms `u16`, two reserved zero bytes |
| `0x1c0 + node` | RuntimeLeaseAck | 24 | boot session `u32`, configuration generation `u32`, epoch `u64`, renewal sequence `u32`, result `u8`, three reserved zero bytes |
| `0x200 + node` | Heartbeat | 32 | boot session `u32`, configuration generation `u32`, capability generation `u32`, heartbeat sequence `u32`, current epoch `u64`, last accepted command sequence `u32`, accepted command `u16`, state flags `u16` |
| `0x240 + node` | FallbackRequest | 16 | boot session `u32`, epoch `u64`, request sequence `u32` |
| `0x280 + node` | FallbackAck | 16 | boot session `u32`, request sequence `u32`, accepted command `u16`, mode `u8`, fault latch `u8`, result `u8`, three reserved zero bytes |
| `0x2c0 + node` | Configuration | 32 | boot session `u32`, configuration generation `u32`, fallback command `u16`, PWM endpoint A `u16`, PWM endpoint B `u16`, direction `u8`, flags `u8`, Runtime Lease ms `u16`, Command Lease ms `u16`, heartbeat period ms `u16`, acknowledgement deadline ms `u16`, normal slew bp/s `u16`, protection slew bp/s `u16`, digest prefix `u32` |
| `0x300 + node` | ConfigurationAck | 16 | boot session `u32`, configuration generation `u32`, digest prefix `u32`, result `u8`, three reserved zero bytes |
| `0x340 + node` | NodeAnnounce | 32 | protocol major `u8`, minor `u8`, lifecycle `u8`, state flags `u8`, boot session `u32`, provisioned identity `u64`, firmware generation `u32`, capability generation `u32`, configuration generation `u32`, announce sequence `u32` |
| `0x380 + node` | CapabilityReport | 24 | boot session `u32`, capability generation `u32`, resource ID `u32`, minimum command `u16`, maximum command `u16`, capability flags `u16`, maximum command rate Hz `u16`, four reserved zero bytes |

IDs `0x081..0x0ff` and `0x3c0..0x7ff` are reserved. Enum value 0 is
invalid. `radiator_split_command` uses basis points `0..10000`, where 0 is the
calibrated intercooler-side endpoint and 10000 is the calibrated radiator-side
endpoint. It is neither measured airflow nor measured position.

## Reconciliation and authority

DiscoveryProbe, NodeAnnounce, and CapabilityReport reconcile only a statically
provisioned node. Discovery grants no authority and unknown identities are
rejected. Configuration reconciliation must precede leases. A controller reboot
changes its boot session and loses authority. Link restoration does not restore
authority.

Runtime and Command Leases are independent controller-local monotonic deadlines.
Each accepted Command refreshes the Command Lease to exactly the configured
Command Lease milliseconds. Rejected, duplicate, stale, wrong-session,
wrong-generation, or wrong-epoch commands do not refresh it. Expiration of
either lease independently selects Controller-Local Fallback, including when the
Command Lease expires while the Runtime Lease remains valid. An acknowledgement
reports electrical application only and never asserts measured mechanism
position.
