#![no_std]
#![no_main]

use defmt::{info, unwrap};
use embassy_executor::Spawner;
use embassy_nrf::peripherals;
use embassy_nrf::{bind_interrupts, rng};
use nrf_sdc::mpsl::MultiprotocolServiceLayer;
use nrf_sdc::{self as sdc, mpsl};
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

#[cfg(not(feature = "nus"))]
use trouble_example_apps::ble_bas_peripheral as ble_peripheral;
#[cfg(feature = "nus")]
use trouble_example_apps::ble_nus_peripheral as ble_peripheral;

/// Default memory allocation for softdevice controller in bytes.
/// - Minimum 2168 bytes,
/// - maximum associated with [task-arena-size](https://docs.embassy.dev/embassy-executor/git/cortex-m/index.html)
#[cfg(not(feature = "nus"))]
const SDC_MEMORY_SIZE: usize = 3312; // bytes
#[cfg(feature = "nus")]
const SDC_MEMORY_SIZE: usize = 5312; // bytes

bind_interrupts!(struct Irqs {
    RNG => rng::InterruptHandler<peripherals::RNG>;
    SWI0_EGU0 => nrf_sdc::mpsl::LowPrioInterruptHandler;
    POWER_CLOCK => nrf_sdc::mpsl::ClockInterruptHandler;
    RADIO => nrf_sdc::mpsl::HighPrioInterruptHandler;
    TIMER0 => nrf_sdc::mpsl::HighPrioInterruptHandler;
    RTC0 => nrf_sdc::mpsl::HighPrioInterruptHandler;
});

#[embassy_executor::task]
async fn mpsl_task(mpsl: &'static MultiprotocolServiceLayer<'static>) -> ! {
    mpsl.run().await
}
/// Build the Softdevice Controller layer to pass to trouble-host
fn build_sdc<'d, const N: usize>(
    p: nrf_sdc::Peripherals<'d>,
    rng: &'d mut rng::Rng<peripherals::RNG>,
    mpsl: &'d MultiprotocolServiceLayer,
    mem: &'d mut sdc::Mem<N>,
) -> Result<nrf_sdc::SoftdeviceController<'d>, nrf_sdc::Error> {
    sdc::Builder::new()?
        .support_adv()?
        .support_peripheral()?
        .peripheral_count(1)?
        .build(p, rng, mpsl, mem)
}

/// Low frequency clock configuration
const LF_CLOCK_CONFIG: mpsl::raw::mpsl_clock_lfclk_cfg_t = mpsl::raw::mpsl_clock_lfclk_cfg_t {
    source: mpsl::raw::MPSL_CLOCK_LF_SRC_RC as u8,
    rc_ctiv: mpsl::raw::MPSL_RECOMMENDED_RC_CTIV as u8,
    rc_temp_ctiv: mpsl::raw::MPSL_RECOMMENDED_RC_TEMP_CTIV as u8,
    accuracy_ppm: mpsl::raw::MPSL_DEFAULT_CLOCK_ACCURACY_PPM as u16,
    skip_wait_lfclk_started: mpsl::raw::MPSL_DEFAULT_SKIP_WAIT_LFCLK_STARTED != 0,
};

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_nrf::init(Default::default());

    // Multiprotocol Service Layer (MPSL)
    let mpsl = {
        let peripherals = mpsl::Peripherals::new(p.RTC0, p.TIMER0, p.TEMP, p.PPI_CH19, p.PPI_CH30, p.PPI_CH31);
        static MPSL: StaticCell<MultiprotocolServiceLayer> = StaticCell::new();
        MPSL.init(unwrap!(mpsl::MultiprotocolServiceLayer::new(
            peripherals,
            Irqs,
            LF_CLOCK_CONFIG
        )))
    };
    spawner.must_spawn(mpsl_task(mpsl));

    let mut rng = rng::Rng::new(p.RNG, Irqs);
    let mut sdc_mem = sdc::Mem::<SDC_MEMORY_SIZE>::new();

    // Softdevice Controller (SDC)
    let sdc = unwrap!({
        let peripherals = sdc::Peripherals::new(
            p.PPI_CH17, p.PPI_CH18, p.PPI_CH20, p.PPI_CH21, p.PPI_CH22, p.PPI_CH23, p.PPI_CH24, p.PPI_CH25, p.PPI_CH26,
            p.PPI_CH27, p.PPI_CH28, p.PPI_CH29,
        );
        build_sdc(peripherals, &mut rng, mpsl, &mut sdc_mem)
    });

    #[cfg(not(feature = "nus"))]
    info!("running Battery Service (BAS) example");
    #[cfg(feature = "nus")]
    info!("running Nordic Uart Service (NUS) example");

    ble_peripheral::run(sdc).await;
}
