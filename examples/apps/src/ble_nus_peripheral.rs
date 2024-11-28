/// Nordic Uart Service (NUS) peripheral example
///
/// The Bluetooth® Low Energy (LE) GATT Nordic UART Service is a custom
/// service that receives and writes data and serves as a bridge to the UART interface.
///
/// https://docs.nordicsemi.com/bundle/ncs-latest/page/nrf/libraries/bluetooth/services/nus.html
use embassy_futures::select::select;
use embassy_time::Timer;
use heapless::Vec;
use trouble_host::prelude::*;

/// Size of L2CAP packets (ATT MTU is this - 4)
const L2CAP_MTU: usize = 251;

/// Max number of connections
const CONNECTIONS_MAX: usize = 1;

/// Max number of L2CAP channels.
const L2CAP_CHANNELS_MAX: usize = 2; // Signal + att

pub const MTU: usize = 120;
// Aligned to 4 bytes + 3 bytes for header
pub const ATT_MTU: usize = MTU + 3;

type Resources<C> = HostResources<C, CONNECTIONS_MAX, L2CAP_CHANNELS_MAX, L2CAP_MTU>;

// GATT Server definition
#[gatt_server]
struct Server {
    nrf_uart: NrfUartService,
}

// NRF UART Service
#[gatt_service(uuid = "6E400001-B5A3-F393-E0A9-E50E24DCCA9E")]
struct NrfUartService {
    /// Write data to the RX Characteristic to send it to the UART interface.
    #[characteristic(uuid = "6E400002-B5A3-F393-E0A9-E50E24DCCA9E", write)]
    rx: Vec<u8, ATT_MTU>,

    /// Enable notifications for the TX Characteristic to receive data from the application.
    /// The application transmits all data that is received over UART as notifications.
    #[characteristic(uuid = "6E400003-B5A3-F393-E0A9-E50E24DCCA9E", notify)]
    tx: Vec<u8, ATT_MTU>,
}

pub async fn run<C>(controller: C)
where
    C: Controller,
{
    // Using a fixed seed means the "random" address will be the same every time the program runs,
    // which can be useful for testing. If truly random addresses are required, a different,
    // dynamically generated seed should be used.
    let address = Address::random([0x41, 0x5A, 0xE3, 0x1E, 0x83, 0xE8]);
    info!("Our address = {:?}", address);

    let mut resources = Resources::new(PacketQos::None);
    let (stack, mut peripheral, _, runner) = trouble_host::new(controller, &mut resources)
        .set_random_address(address)
        .build();

    info!("Starting advertising and GATT service");
    let server = Server::new_with_config(
        stack,
        GapConfig::Peripheral(PeripheralConfig {
            name: "TrouBLE NUS",
            appearance: &appearance::UNKNOWN,
        }),
    )
    .unwrap();
    let ble_background_tasks = select(ble_task(runner), gatt_task(&server));
    let app_task = async {
        loop {
            match advertise("Trouble Example", &mut peripheral).await {
                Ok(conn) => {
                    // set up tasks when the connection is established to a central, so they don't run when no one is connected.
                    let connection_task = conn_task(&server, &conn);
                    let counter_task = counter_task(&server, &conn);
                    // run until any task ends (usually because the connection has been closed),
                    // then return to advertising state.
                    select(connection_task, counter_task).await;
                }
                Err(_) => info!("[adv] error"),
            }
        }
    };
    select(ble_background_tasks, app_task).await;
}

async fn ble_task<C: Controller>(mut runner: Runner<'_, C>) -> Result<(), BleHostError<C::Error>> {
    runner.run().await
}

async fn gatt_task<C: Controller>(server: &Server<'_, '_, C>) -> Result<(), BleHostError<C::Error>> {
    server.run().await
}

async fn conn_task<C: Controller>(
    server: &Server<'_, '_, C>,
    conn: &Connection<'_>,
) -> Result<(), BleHostError<C::Error>> {
    let tx = &server.nrf_uart.tx;
    let rx = &server.nrf_uart.rx;

    // Keep connection alive
    loop {
        match conn.next().await {
            ConnectionEvent::Disconnected { reason } => {
                info!("[gatt] disconnected: {:?}", reason);
                break;
            }
            ConnectionEvent::Gatt { event, .. } => match event {
                GattEvent::Read { value_handle } => {
                    if value_handle == rx.handle {
                        let value = server.get(&rx).unwrap();
                        info!("[gatt] Read Event to rx_buf Characteristic: {:?}", value.len());
                    } else if value_handle == tx.handle {
                        let value = server.get(&tx).unwrap();
                        info!("[gatt] Read Event to tx_buf Characteristic: {:?}", value.len());
                    }
                }
                GattEvent::Write { value_handle } => {
                    if value_handle == rx.handle {
                        let value = server.get(&rx).unwrap();
                        info!("[gatt] Write Event to rx_buf Characteristic: {:?}", value.len());
                    } else if value_handle == tx.handle {
                        let value = server.get(&tx).unwrap();
                        info!("[gatt] Write Event to tx_buf Characteristic: {:?}", value.len());
                    }
                }
            },
        }
    }
    Ok(())
}

/// Create an advertiser to use to connect to a BLE Central, and wait for it to connect.
async fn advertise<'a, C: Controller>(
    name: &'a str,
    peripheral: &mut Peripheral<'a, C>,
) -> Result<Connection<'a>, BleHostError<C::Error>> {
    let name = if name.len() > 22 {
        let truncated_name = &name[..22];
        info!("Name truncated to {}", truncated_name);
        truncated_name
    } else {
        name
    };
    let mut advertiser_data = [0; 31];
    AdStructure::encode_slice(
        &[
            AdStructure::Flags(LE_GENERAL_DISCOVERABLE | BR_EDR_NOT_SUPPORTED),
            AdStructure::ServiceUuids16(&[Uuid::Uuid16([0x0f, 0x18])]),
            AdStructure::CompleteLocalName(name.as_bytes()),
        ],
        &mut advertiser_data[..],
    )?;
    let mut advertiser = peripheral
        .advertise(
            &Default::default(),
            Advertisement::ConnectableScannableUndirected {
                adv_data: &advertiser_data[..],
                scan_data: &[],
            },
        )
        .await?;
    info!("[adv] advertising");
    let conn = advertiser.accept().await?;
    info!("[adv] connection established");
    Ok(conn)
}

/// Example task to use the BLE notifier interface.
async fn counter_task<C: Controller>(server: &Server<'_, '_, C>, conn: &Connection<'_>) {
    let mut tick: u8 = 0;
    let mut buf = Vec::<u8, ATT_MTU>::from_slice(&[0; ATT_MTU]).unwrap();
    let tx = &server.nrf_uart.tx;
    loop {
        tick = tick.wrapping_add(1);
        info!("[adv] notifying connection of tick {}", tick);
        buf[0] = tick;
        if server.notify(tx, conn, &buf).await.is_err() {
            info!("[adv] error notifying connection");
            break;
        };
        Timer::after_secs(2).await;
    }
}
