use embassy_futures::join::join;
use embassy_futures::select::select;
use trouble_host::prelude::AdStructure;
use trouble_host::prelude::*;

/// Max number of connections
const CONNECTIONS_MAX: usize = 1;

/// Max number of L2CAP channels.
const L2CAP_CHANNELS_MAX: usize = 3; // Signal + att + CoC

/// PSM CID to be published to the client
const PSM_CID: u16 = 0x00C0;

/// GATT Server
#[gatt_server]
struct Server {
    psm_service: PsmService,
}

/// PSM service (to publish PSM CID to L2CAP client upon connection)
/// The PSM Service UUID is specific to the services iOS/macOS App is scanning for
/// Here, we use value from https://github.com/paulw11/L2CapDemo (L2CapDemo/L2CapDemo/Constants.swift)
/// The PSM Characteristic UUID is predefined https://developer.apple.com/documentation/corebluetooth/cbuuidl2cappsmcharacteristicstring
/// iOS/macOS only accepts L2CAP PSM values in range 0x0040 - 0x00FF
#[gatt_service(uuid = "12E61727-B41A-436F-B64D-4777B35F2294")]
struct PsmService {
    #[characteristic(uuid = "ABDD3056-28FA-441D-A470-55A75A52553A", read, indicate, value = PSM_CID)]
    psm: u16,
}

pub async fn run<C, const L2CAP_MTU: usize>(controller: C)
where
    C: Controller,
{
    // Hardcoded peripheral address
    let address: Address = Address::random([0xff, 0x8f, 0x1a, 0x05, 0xe4, 0xff]);
    info!("Our address = {:?}", address);

    let mut resources: HostResources<CONNECTIONS_MAX, L2CAP_CHANNELS_MAX, L2CAP_MTU> = HostResources::new();
    let stack = trouble_host::new(controller, &mut resources).set_random_address(address);
    let Host {
        mut peripheral,
        mut runner,
        ..
    } = stack.build();

    let mut adv_data = [0; 31];
    AdStructure::encode_slice(
        &[AdStructure::Flags(LE_GENERAL_DISCOVERABLE | BR_EDR_NOT_SUPPORTED)],
        &mut adv_data[..],
    )
    .unwrap();

    // Advertise the PSM service the iOS/macOS client is scanning for
    let mut scan_data = [0; 31];
    AdStructure::encode_slice(
        &[
            AdStructure::CompleteLocalName(b"Trouble"),
            AdStructure::ServiceUuids128(&[0x12E61727_B41A_436F_B64D_4777B35F2294_u128.to_le_bytes()]),
        ],
        &mut scan_data[..],
    )
    .unwrap();

    let server = Server::new_with_config(GapConfig::Peripheral(PeripheralConfig {
        name: "Trouble L2CAP Server",
        appearance: &appearance::sensor::GENERIC_SENSOR,
    }))
    .unwrap();

    let _ = join(runner.run(), async {
        loop {
            info!("Advertising, waiting for connection...");
            let advertiser = peripheral
                .advertise(
                    &Default::default(),
                    Advertisement::ConnectableScannableUndirected {
                        adv_data: &adv_data[..],
                        scan_data: &scan_data[..],
                    },
                )
                .await
                .unwrap();

            let conn = advertiser
                .accept()
                .await
                .unwrap()
                .with_attribute_server(&server)
                .unwrap();

            info!("Connection established");

            let gatt_fut = gatt_task(&conn);
            let l2cap_fut = l2cap_task::<_, L2CAP_MTU, PSM_CID>(&conn, &stack);
            select(gatt_fut, l2cap_fut).await;
        }
    })
    .await;
}

/// Task to handle Gatt events
async fn gatt_task(conn: &GattConnection<'_, '_>) {
    loop {
        match conn.next().await {
            GattConnectionEvent::Disconnected { reason } => {
                info!("[gatt] disconnected: {:?}", reason);
                break;
            }
            GattConnectionEvent::Gatt { event } => match event {
                Ok(event) => match event.accept() {
                    Ok(reply) => reply.send().await,
                    Err(e) => warn!("[gatt] error accepting event: {:?}", e),
                },
                Err(e) => warn!("[gatt] error processing event: {:?}", e),
            },
        }
    }
}

/// Task to handle L2CAP data streaming
async fn l2cap_task<'a, C: Controller, const L2CAP_MTU: usize, const PSM: u16>(
    conn: &GattConnection<'_, '_>,
    stack: &'a Stack<'a, C>,
) {
    let mut channel = L2capChannel::accept(&stack, conn.raw(), &[PSM], &Default::default())
        .await
        .unwrap();
    info!("[l2cap] channel accepted");

    let mut buf = [0; L2CAP_MTU];
    loop {
        match channel.receive(&stack, &mut buf).await {
            Ok(len) => {
                let rx_data = &buf[..len];
                info!("[l2cap] received: {:?}", rx_data);

                // echo received data
                match channel.send::<_, L2CAP_MTU>(&stack, rx_data).await {
                    Ok(_) => {
                        info!("[l2cap] sending: {:?}", rx_data);
                    }
                    Err(e) => warn!("[l2cap] error sending data: {:?}", e),
                }
            }
            Err(e) => warn!("[l2cap] error receiving data: {:?}", e),
        }
    }
}
