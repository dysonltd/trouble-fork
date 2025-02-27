use core::any::Any;
use bt_hci::cmd::AsyncCmd;
use embassy_futures::join::join;
use embassy_time::{Duration, Instant, Timer};
use trouble_host::prelude::*;

use bt_hci::cmd::le::{LeSetPhy, LeReadPhyReturn, LeSetDataLength, LeReadPhy, LeReadBufferSize,
                      LeReadMaxDataLength, LeReadLocalSupportedFeatures,
                      LeWriteSuggestedDefaultDataLength, LeReadSuggestedDefaultDataLength};
use bt_hci::controller::{ControllerCmdAsync, ControllerCmdSync};
use bt_hci::param::{AllPhys, ConnHandle, PhyMask, PhyOptions};
use embedded_io::ErrorType;

/// Max number of connections
const CONNECTIONS_MAX: usize = 1;

/// Max number of L2CAP channels.
const L2CAP_CHANNELS_MAX: usize = 3; // Signal + att + CoC

const MY_L2CAP_MTU: usize = 256;

pub async fn run<C, const MY_L2CAP_MTU: usize>(controller: C)
where
    C: Controller
    + ControllerCmdAsync<LeSetPhy>
    + ControllerCmdSync<LeSetDataLength>
    + ControllerCmdSync<LeReadLocalSupportedFeatures>
    + ControllerCmdSync<LeWriteSuggestedDefaultDataLength>
    + ControllerCmdSync<LeReadSuggestedDefaultDataLength>
    + ControllerCmdSync<LeReadMaxDataLength>,
{
    // Using a fixed "random" address can be useful for testing. In real scenarios, one would
    // use e.g. the MAC 6 byte array as the address (how to get that varies by the platform).
    let address: Address = Address::random([0xff, 0x8f, 0x1b, 0x05, 0xe4, 0xff]);
    info!("Our address = {:?}", address);

    let mut resources: HostResources<CONNECTIONS_MAX, L2CAP_CHANNELS_MAX, MY_L2CAP_MTU> = HostResources::new();
    let stack = trouble_host::new(controller, &mut resources).set_random_address(address);

    let Host {
        mut central,
        mut runner,
        ..
    } = stack.build();

    // NOTE: Modify this to match the address of the peripheral you want to connect to.
    // Currently, it matches the address used by the peripheral examples
    let target: Address = Address::random([0xff, 0x8f, 0x1a, 0x05, 0xe4, 0xff]);

    let config = ConnectConfig {
        connect_params: ConnectParams{
            min_connection_interval: Duration::from_micros(7_500),
            max_connection_interval: Duration::from_micros(7_500),
            max_latency: 0,
            event_length: Duration::from_millis(30),
            supervision_timeout: Duration::from_millis(150),
            ..Default::default()
        },
        scan_config: ScanConfig {
            // active: true,
            filter_accept_list: &[(target.kind, &target.addr)],
            phys: PhySet::M2,
            // interval: Duration::from_secs(1),
            // window: Duration::from_secs(1),
            // timeout: Duration::from_secs(0),
            ..Default::default()
        },
    };

    info!("Scanning for peripheral...");
    let _ = join(runner.run(), async {
        loop {
            // Check that the controller used supports the necessary features for high throughput.
            let res = stack.command(LeReadLocalSupportedFeatures::new()).await.unwrap();
            assert!(res.supports_le_data_packet_length_extension());
            assert!(res.supports_le_2m_phy());

            // let res = stack.command(LeReadMaxDataLength::new()).await.unwrap();
            // info!("LeReadMaxDataLength: {:?}", res);
            //
            // let res = stack.command(LeReadSuggestedDefaultDataLength::new()).await.unwrap();
            // info!("LeReadSuggestedDefaultDataLength: {:?}", res);
            //
            // match stack.command(LeWriteSuggestedDefaultDataLength::new(251, 2120)).await {
            //     Ok(_) => { info!("LeWriteSuggestedDefaultDataLength OK"); }
            //     Err(e) => { info!("LeWriteSuggestedDefaultDataLength Err: {:?}", e); }
            // }
            //
            // let res = stack.command(LeReadSuggestedDefaultDataLength::new()).await.unwrap();
            // info!("LeReadSuggestedDefaultDataLength 2: {:?}", res);

            let conn = central.connect(&config).await.unwrap();

            // let res = stack.command(LeSetDataLength::new(conn.handle(), 251, 2120)).await;
            // match res {
            //     Ok(_) => {
            //         info!("LeSetDataLength OK");
            //     }
            //     Err(e) => {
            //         info!("LeSetDataLength error: {:?}", e);
            //     }
            // }
            //
            let phy_mask = PhyMask::new().set_le_2m_preferred(true);
            stack.async_command(LeSetPhy::new(conn.handle(), AllPhys::default(), phy_mask.clone(), phy_mask, PhyOptions::S2CodingPreferred)).await.unwrap();

            let res = stack.command(LeReadBufferSize::new()).await.unwrap();
            info!("LeReadBufferSize: {:?}", res);

            info!("Connected, creating l2cap channel");
            const PAYLOAD_LEN: usize = 494;
            let l2cap_channel_config = L2capChannelConfig {
                mtu: 251,
                flow_policy: CreditFlowPolicy::Every(50),
                initial_credits: Some(50),
            };
            let mut ch1 = L2capChannel::create(&stack, &conn, 0x2349, &Default::default())
                .await
                .unwrap();
            info!("New l2cap channel created, sending some data!");

            let start = Instant::now();

            for i in 0..10 {
                let tx = [i+0x41; PAYLOAD_LEN];
                ch1.send::<_, MY_L2CAP_MTU>(&stack, &tx).await.unwrap();
            }

            let duration = start.elapsed();
            info!("L2cap channel send duration: {:?}", duration.as_millis());

            info!("Sent data, waiting for them to be sent back");
            let mut rx = [0; PAYLOAD_LEN];
            for i in 0..10 {
                let len = ch1.receive(&stack, &mut rx).await.unwrap();
                assert_eq!(len, rx.len());
                assert_eq!(rx, [i; PAYLOAD_LEN]);
            }

            info!("Received successfully!");

            Timer::after(Duration::from_secs(60)).await;
        }
    })
    .await;
}
