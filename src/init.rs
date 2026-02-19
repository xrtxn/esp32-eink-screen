use display_interface::WriteOnlyDataCommand;
use display_interface_spi::SPIInterface;
use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{InputPin as EhalInputPin, OutputPin as EhalOutputPin};
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_hal::gpio::{InputPin, OutputPin};
use esp_hal::peripherals::SPI2;
use esp_hal::rtc_cntl::Rtc;
use esp_hal::{
    delay::Delay,
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
    spi::{
        master::{Config, Spi},
        Mode,
    },
    time::Rate,
};
use weact_studio_epd::graphics::{Display420BlackWhite, DisplayRotation};
use weact_studio_epd::WeActStudio420BlackWhiteDriver;

pub(crate) async fn init_display(
    sclk_pin: impl OutputPin + 'static,
    mosi_pin: impl OutputPin + 'static,
    spi_pin: SPI2<'static>,
    dc_pin: impl OutputPin + 'static,
    rst_pin: impl OutputPin + 'static,
    busy_pin: impl InputPin + 'static,
    cs_pin: impl OutputPin + 'static,
) -> (Display420BlackWhite, crate::EpdDriver) {
    let spi_bus = Spi::new(
        spi_pin,
        Config::default()
            .with_frequency(Rate::from_mhz(4))
            .with_mode(Mode::_0),
    )
    .unwrap()
    .with_sck(sclk_pin)
    .with_mosi(mosi_pin);

    let dc = Output::new(dc_pin, Level::Low, OutputConfig::default());
    let rst = Output::new(rst_pin, Level::High, OutputConfig::default());
    let busy = Input::new(busy_pin, InputConfig::default().with_pull(Pull::None));
    let cs = Output::new(cs_pin, Level::High, OutputConfig::default());

    log::info!("Intializing SPI Device...");
    let spi_device =
        ExclusiveDevice::new(spi_bus, cs, Delay::new()).expect("SPI device initialize error");
    let spi_interface = SPIInterface::new(spi_device, dc);

    log::info!("Intializing EPD...");
    let mut driver = WeActStudio420BlackWhiteDriver::new(spi_interface, busy, rst, Delay::new());
    let mut display = Display420BlackWhite::new();
    // set it to be longer not wider
    display.set_rotation(DisplayRotation::Rotate270);
    driver.init().unwrap();
    log::info!("EPD initialized!");
    (display, driver)
}
