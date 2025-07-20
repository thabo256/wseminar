#![no_std]
#![no_main]

use bsp::entry;
use bsp::hal;
use core::{convert::Infallible, panic};
use cortex_m::prelude::*;
use defmt::*;
use defmt_rtt as _;
use embedded_hal::digital::{InputPin, OutputPin, PinState};
use fugit::ExtU32;
use panic_probe as _;
use usb_device::class_prelude::*;
use usb_device::prelude::*;
use usbd_human_interface_device::page::Keyboard;
use usbd_human_interface_device::prelude::*;

// Provide an alias for our BSP so we can switch targets quickly.
use rp_pico as bsp;

use bsp::hal::{
    clocks::{init_clocks_and_plls, Clock},
    pac,
    sio::Sio,
    watchdog::Watchdog,
};

const NUM_COLS: usize = 4;
const NUM_ROWS: usize = 4;

#[entry]
fn main() -> ! {
    info!("Program start");
    let mut pac = pac::Peripherals::take().unwrap();
    let core = pac::CorePeripherals::take().unwrap();
    let mut watchdog = Watchdog::new(pac.WATCHDOG);
    let sio = Sio::new(pac.SIO);

    // External high-speed crystal on the pico board is 12Mhz
    let external_xtal_freq_hz = 12_000_000u32;
    let clocks = init_clocks_and_plls(
        external_xtal_freq_hz,
        pac.XOSC,
        pac.CLOCKS,
        pac.PLL_SYS,
        pac.PLL_USB,
        &mut pac.RESETS,
        &mut watchdog,
    )
    .ok()
    .unwrap();

    let timer = hal::Timer::new(pac.TIMER, &mut pac.RESETS, &clocks);

    let mut delay = cortex_m::delay::Delay::new(core.SYST, clocks.system_clock.freq().to_Hz());

    let pins = bsp::Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    // Set up keyboard matrix pins.
    let mut rows: [&mut dyn InputPin<Error = Infallible>; NUM_ROWS] = [
        &mut pins.gpio10.into_pull_down_input(),
        &mut pins.gpio11.into_pull_down_input(),
        &mut pins.gpio12.into_pull_down_input(),
        &mut pins.gpio13.into_pull_down_input(),
    ];

    let mut cols: [&mut dyn OutputPin<Error = Infallible>; NUM_COLS] = [
        &mut pins.gpio21.into_push_pull_output(),
        &mut pins.gpio20.into_push_pull_output(),
        &mut pins.gpio19.into_push_pull_output(),
        &mut pins.gpio18.into_push_pull_output(),
    ];

    //USB
    let usb_bus = UsbBusAllocator::new(hal::usb::UsbBus::new(
        pac.USBCTRL_REGS,
        pac.USBCTRL_DPRAM,
        clocks.usb_clock,
        true,
        &mut pac.RESETS,
    ));

    let mut keyboard = UsbHidClassBuilder::new()
        .add_device(
            usbd_human_interface_device::device::keyboard::NKROBootKeyboardConfig::default(),
        )
        .build(&usb_bus);

    let mut usb_dev = UsbDeviceBuilder::new(&usb_bus, UsbVidPid(0x1209, 0x0001))
        .strings(&[StringDescriptors::default()
            .manufacturer("thabo")
            .product("steno keyboard")
            .serial_number("TEST")])
        .unwrap()
        .build();

    let mut led_pin = pins.led.into_push_pull_output();

    let mut input_count_down = timer.count_down();
    input_count_down.start(10.millis());

    let mut tick_count_down = timer.count_down();
    tick_count_down.start(1.millis());

    loop {
        //Poll the keys every 10ms
        if input_count_down.wait().is_ok() {
            let scan = scan(&mut rows, &mut cols, &mut delay);

            let keys = get_keys(&scan);

            match keyboard.device().write_report(keys) {
                Err(UsbHidError::WouldBlock) => {}
                Err(UsbHidError::Duplicate) => {}
                Ok(_) => {}
                Err(e) => {
                    panic!("Failed to write keyboard report: {:?}", e)
                }
            };
        }

        //Tick once per ms
        if tick_count_down.wait().is_ok() {
            match keyboard.tick() {
                Err(UsbHidError::WouldBlock) => {}
                Ok(_) => {}
                Err(e) => {
                    panic!("Failed to process keyboard tick: {:?}", e)
                }
            };
        }

        if usb_dev.poll(&mut [&mut keyboard]) {
            match keyboard.device().read_report() {
                Err(UsbError::WouldBlock) => {
                    //do nothing
                }
                Err(e) => {
                    panic!("Failed to read keyboard report: {:?}", e)
                }
                Ok(leds) => {
                    led_pin.set_state(PinState::from(leds.caps_lock)).ok();
                }
            }
        }
    }
}

fn scan(
    rows: &mut [&mut dyn InputPin<Error = Infallible>; NUM_ROWS],
    columns: &mut [&mut dyn embedded_hal::digital::OutputPin<Error = Infallible>; NUM_COLS],
    delay: &mut cortex_m::delay::Delay,
) -> [[bool; NUM_COLS]; NUM_ROWS] {
    let mut raw_matrix = [[false; NUM_ROWS]; NUM_COLS];

    for (gpio_col, matrix_col) in columns.iter_mut().zip(raw_matrix.iter_mut()) {
        gpio_col.set_high().unwrap();
        delay.delay_us(1);

        for (gpio_row, matrix_row) in rows.iter_mut().zip(matrix_col.iter_mut()) {
            *matrix_row = gpio_row.is_high().unwrap();
        }

        gpio_col.set_low().unwrap();
    }

    raw_matrix
}

const KEY_MAP: [[Keyboard; NUM_COLS]; NUM_ROWS] = [
    [Keyboard::A, Keyboard::B, Keyboard::C, Keyboard::D],
    [Keyboard::E, Keyboard::F, Keyboard::G, Keyboard::H],
    [Keyboard::I, Keyboard::J, Keyboard::K, Keyboard::L],
    [Keyboard::M, Keyboard::N, Keyboard::O, Keyboard::P],
];

fn get_keys(scan: &[[bool; NUM_COLS]; NUM_ROWS]) -> [Keyboard; NUM_COLS * NUM_ROWS] {
    let mut keys = [Keyboard::NoEventIndicated; NUM_COLS * NUM_ROWS];
    let mut index = 0;

    for (row_index, row) in scan.iter().enumerate() {
        for (col_index, &pressed) in row.iter().enumerate() {
            if pressed {
                keys[index] = KEY_MAP[row_index][col_index];
                index += 1;
            }
        }
    }

    keys
}
