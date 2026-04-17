use display_interface_spi::SPIInterface;
use embassy_time::Delay;
use embedded_hal_bus::spi::ExclusiveDevice;
use esp_hal::gpio::{InputPin, OutputPin};
use esp_hal::peripherals::SPI2;
use esp_hal::{
    gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull},
    spi::{
        Mode,
        master::{Config, Spi},
    },
    time::Rate,
};
use weact_studio_epd::WeActStudio420BlackWhiteDriver;
use weact_studio_epd::graphics::{Display420BlackWhite, DisplayRotation};

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
    .with_mosi(mosi_pin)
    .into_async();

    let dc = Output::new(dc_pin, Level::Low, OutputConfig::default());
    let rst = Output::new(rst_pin, Level::High, OutputConfig::default());
    let busy = Input::new(busy_pin, InputConfig::default().with_pull(Pull::None));
    let cs = Output::new(cs_pin, Level::High, OutputConfig::default());

    defmt::info!("Initializing SPI Device...");
    let spi_device = ExclusiveDevice::new(spi_bus, cs, Delay).expect("SPI device initialize error");
    let spi_interface = SPIInterface::new(spi_device, dc);

    defmt::info!("Initializing EPD...");
    let mut driver = WeActStudio420BlackWhiteDriver::new(spi_interface, busy, rst, Delay);
    let mut display = Display420BlackWhite::new();
    // set it to be longer not wider
    display.set_rotation(DisplayRotation::Rotate270);
    driver.init().await.unwrap();
    defmt::info!("EPD initialized!");
    (display, driver)
}
