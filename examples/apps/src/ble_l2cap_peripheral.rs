use embassy_futures::join::join;
use embassy_time::{Duration, Timer};
use trouble_host::prelude::*;

use crate::common::PSM_L2CAP_EXAMPLES;

/// Max number of connections
const CONNECTIONS_MAX: usize = 1;

/// Max number of L2CAP channels.
const L2CAP_CHANNELS_MAX: usize = 3; // Signal + att + CoC

pub async fn run<C>(controller: C)
where
    C: Controller,
{
    // Hardcoded peripheral address
    let address: Address = Address::random([0xff, 0x8f, 0x1a, 0x05, 0xe4, 0xff]);
    info!("Our address = {:?}", address);

    let mut resources: HostResources<DefaultPacketPool, CONNECTIONS_MAX, L2CAP_CHANNELS_MAX> = HostResources::new();
    let stack = trouble_host::new(controller, &mut resources).set_random_address(address);
    let Host {
        mut peripheral,
        mut runner,
        ..
    } = stack.build();

    let mut adv_data = [0; 31];
    let adv_data_len = AdStructure::encode_slice(
        &[AdStructure::Flags(LE_GENERAL_DISCOVERABLE | BR_EDR_NOT_SUPPORTED)],
        &mut adv_data[..],
    )
    .unwrap();

    let mut scan_data = [0; 31];
    let scan_data_len =
        AdStructure::encode_slice(&[AdStructure::CompleteLocalName(b"Trouble")], &mut scan_data[..]).unwrap();

    let _ = join(runner.run(), async {
        loop {
            info!("Advertising, waiting for connection...");
            let advertiser = peripheral
                .advertise(
                    &Default::default(),
                    Advertisement::ConnectableScannableUndirected {
                        adv_data: &adv_data[..adv_data_len],
                        scan_data: &scan_data[..scan_data_len],
                    },
                )
                .await
                .unwrap();
            let conn = advertiser.accept().await.unwrap();

            info!("Connection established");

            let config = L2capChannelConfig {
                mtu: Some(PAYLOAD_LEN as u16),
                ..Default::default()
            };
            let mut ch1 = L2capChannel::accept(&stack, &conn, &[PSM_L2CAP_EXAMPLES], &config)
                .await
                .unwrap();

            info!("L2CAP channel accepted");

            // Size of payload we're expecting
            const PAYLOAD_LEN: usize = 27;
            let mut rx = [0; PAYLOAD_LEN];
            for i in 0..10 {
                let len = ch1.receive(&stack, &mut rx).await.unwrap();
                assert_eq!(len, rx.len());
                assert_eq!(rx, [i; PAYLOAD_LEN]);
            }

            info!("L2CAP data received, echoing");
            Timer::after(Duration::from_secs(1)).await;
            for i in 0..10 {
                let tx = [i; PAYLOAD_LEN];
                ch1.send(&stack, &tx).await.unwrap();
            }
            info!("L2CAP data echoed");

            Timer::after(Duration::from_secs(60)).await;
        }
    })
    .await;
}
