# AirPlay implementation and hardware investigation

Status at the end of the November 10–11, 2026 hospital test window:
discovery and PIN pairing have worked, but sustained mirroring to the physical
Apple TV has **not** been demonstrated. Live testing stopped when the operator
left the TV. The P2P GO experiment is prepared for a future window and has not been
tested against that receiver.

## What is implemented

- A native Wayland AirPlay window with TV discovery, PIN entry, screen/audio
  capture, mute, stop, manual addresses and connection errors.
- Discovery details exposing Bonjour and Bluetooth metadata, optional signal
  sorting, tracking of one Bluetooth identity and stale-reading indicators.
- A separate privileged AWDL radio helper. Pairing, capture, media and the GUI
  remain unprivileged. Closing the app releases the radio lease and removes
  its temporary pairing material.
- Systemd-owned recovery, bounded diagnostic variants and a 34-case battery.
  The new autonomous mode selects an exact TV name and device ID, generates
  synthetic video, waits for a PIN when necessary, records session stages and
  advances without screen-selection dialogs or terminal verdict prompts.

The autonomous runner never reports a physical-TV pass from sender state
alone. Its `unverified` verdict is deliberate: sustained TCP delivery and a
streaming status cannot show what the display actually rendered.

## Evidence and interpretation

The laptop has Intel BE211 (`8086:e440`), the `iwlmld` driver, kernel 7.3.3 and
firmware `107.1b444747.0`. The target identifies as AppleTV14,1. An iPhone
successfully mirrored to it. The laptop and iPhone had guest-network addresses in the
same `/16`; this did not establish a usable infrastructure path between them.

| Experiment | Observation | What it establishes |
| --- | --- | --- |
| Early passive-monitor sessions | Discovery, PIN, screen selection and a short blank TV screen; first large video write could time out | Discovery and pairing alone do not validate throughput |
| Collected four-case batch | Baseline and 1 ms pacing paired, then timed out reading encrypted post-control `/info`; TCP smoothed RTT reached about 4–5 seconds. Other cases did not discover the target | Those failures occurred before media setup, not in the encoder |
| Autonomous baseline | Exact target selected without clicks; transient pairing/reconnect timed out before PIN or media. Of 218 incoming AWDL data frames from the TV addressed to the laptop, 181 had the retry flag. Consecutive repetition reached 19 frames, with signal around −38 dBm | Strong evidence to investigate missing receive acknowledgements; not a measured packet-loss percentage |
| Autonomous QoS No Ack variant | No target Bonjour discovery within 90 seconds; the same AWDL peer remained audible | This run did not establish a session or demonstrate a fix; discovery variation prevents a clean throughput comparison |
| Intel channel 149 rate correction | Corrected injected OFDM rate reached firmware assertion `0x13266` | Do not repeat this crash without a new, specific driver/firmware explanation |
| IBSS receive-filter experiment | Peers became visible but firmware later asserted `0xB70` | Broad receive-filter changes are not a demonstrated solution |
| P2P GO context | Offline setup/cleanup tests prepared; no physical run | Candidate only, not a working BE211 mode |

The four-case batch and the two autonomous cases restored guest Wi-Fi and
showed no firmware assertion. The separate earlier driver experiments did
crash firmware and exercised recovery. The last live radio service is stopped.

Retry counts come from locally received 802.11 headers filtered by AWDL BSSID,
destination MAC and source peer. They contain no application payload. A second
radio observing ACKs independently would provide stronger evidence than the
local monitor; do not describe these counters as an independent air capture.

Raw event, TCP and kernel evidence remains outside Git in the operator's
private `airplay-results-20261110-233716` and `airplay-autonomous-ack-01`
directories. They contain nearby-device and network identifiers. The battery
records immutable package paths in each run's `plan.json`.

## Why the next test concerns receive acknowledgements

[Apple's deployment guidance](https://support.apple.com/en-gb/guide/deployment/-dep9151c4ace/web)
documents Bluetooth-assisted discovery and peer Wi-Fi. Bluetooth proximity
does not itself provide the media transport, and forcing channel 149 is not
the only possible route.

[OWL's hardware requirements](https://github.com/seemoo-lab/owl#requirements)
and [Owly's radio modes](https://github.com/t4t5/owly#modes) explain why received
unicast needs hardware ACKs. A passive monitor can hear discovery while the
peer repeatedly retransmits data. [Owly issue #4](https://github.com/t4t5/owly/issues/4)
reports this on an AX211; that supports investigation but does not prove the
same failure mechanism on every Intel chip.

[PR #102](https://github.com/t4t5/owly/pull/102) proposes setting QoS No Ack in
the outgoing frame because newer Intel transmit commands omit the radiotap
no-ACK request. This addresses outgoing firmware retry queues, not the absence
of ACKs for incoming frames. The isolated `qos-noack` case retains that
distinction.

[PR #21](https://github.com/t4t5/owly/pull/21) reports successful AirDrop
transfers using a secondary channel-6 P2P GO beside an associated station on
one AX211 configuration. BE211 advertises the required two-channel interface
combination. The `p2p-go` experiment checks channel permission and concurrency,
uses the same random local MAC for its GO and monitor, preserves the station,
and journals cleanup. It runs no DHCP, routing or authentication service on
the GO and does not patch the driver or override RF restrictions. Its offline
tests cover partial failure and rejection of unsupported/prohibited setups.

## Protocol sources and limits

[Apple's MFi program](https://mfi.apple.com/en/how-it-works) describes licensed
specifications and certification. No manufacturer-only mirroring specification
was obtained or claimed to have been consulted. Public
[LG hospitality documentation](https://www.lg.com/us/business/airplay-for-hospitality)
describes a licensed integration, not the wire behavior of this Apple TV.

The [OpenAirPlay specification](https://openairplay.github.io/airplay-spec/)
states an Apple TV software 5.0/iOS 5.1 baseline. It cannot settle all current
HAP or timing behavior. Independent [PyATV pairing code](https://github.com/postlund/pyatv/blob/master/pyatv/protocols/airplay/auth/hap.py)
and [encrypted record framing](https://github.com/postlund/pyatv/blob/master/pyatv/auth/hap_session.py)
were compared with the pinned sender. No specific mismatch explaining the
observed timeout was identified. [UxPlay's cryptography notes](https://github.com/FDH2/UxPlay/wiki/crypto)
describe an independently useful receiver, but passing legacy pairing/FairPlay
tests would not certify current Apple TV HAP behavior. UxPlay interoperability
was not physically tested in this window.

## Resume testing

Offline checks completed during this handoff: 20 native-app tests, 5 radio
helper tests, 19 battery harness tests, and 3 P2P GO setup/recovery tests plus
the GO beacon-layout test. The root-flake package builds also exercise the
patched AWDL protocol. These are component checks; none certify TV playback.

Build before the next visit; this does not start a radio session:

```sh
nix build .#airplay-test-battery --no-update-lock-file --out-link .work/airplay/test-battery
.work/airplay/test-battery/bin/sekka-airplay-battery plan --case p2p-go --case baseline-mcs7 --mirror
```

With the intended TV available, read its name and device ID from discovery
details and substitute both below. Run as the desktop user:

```sh
.work/airplay/test-battery/bin/sekka-airplay-battery run --autonomous \
  --target 'EXACT TV NAME' --device-id 'AA:BB:CC:DD:EE:FF' \
  --case p2p-go --case baseline-mcs7 --output "$HOME/airplay-next-results"
```

Linux may ask once for administrator authentication. Enter any TV PIN in the
AirPlay window; never pass it in shell arguments or logs. The runner requires
a unique matching name, device ID and scoped AWDL endpoint. It attempts once
per case, stops on a sender error or deadline, checks recovery and advances.
If a TV rotates its identity, refresh discovery instead of weakening selection.

Compare incoming retry counts, received/ACKed TCP bytes and exact session
stage. If the GO case cannot establish reception, retain that result rather
than treating it as an encoder failure. Do not repeat known firmware-crash
cases merely to fill a matrix. Once transport works, verify at least 60 seconds
of visible moving video and sound separately, then examine codec, timing and
audio interoperability. No ISO, VM installation or physical-TV qualification
is implied by the component tests.

See [DEVELOPERS.md](../DEVELOPERS.md#prepared-hardware-test-battery) for the
complete catalog, recovery model and build/check commands.
