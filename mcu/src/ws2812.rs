// Note: based on https://github.com/smart-leds-rs/ws2812-spi-rs

use esp_hal::{Async, DriverMode};
use smart_leds::RGB8;

pub const WS2812_RESET_BYTES: usize = 140;

#[allow(non_camel_case_types)]
pub struct WS2812_Spi<'spi, 'buffer, Mode: DriverMode, const B: usize> {
    pub spi: esp_hal::spi::master::SpiDmaBus<'spi, Mode>,
    pub buffer: &'buffer mut [u8; B],
}

impl<'spi, 'buffer, Mode: DriverMode, const B: usize> WS2812_Spi<'spi, 'buffer, Mode, B> {
    #[allow(unused)]
    pub fn write<const N: usize>(&mut self, pixels: &[RGB8; N]) -> Result<(), esp_hal::spi::Error> {
        assert!(B >= 12 * N + WS2812_RESET_BYTES);

        encode_sequence(self.buffer, pixels);

        self.spi.write(self.buffer)?;

        Ok(())
    }
}

impl<'spi, 'buffer, const B: usize> WS2812_Spi<'spi, 'buffer, Async, B> {
    pub async fn write_async<const N: usize>(
        &mut self,
        pixels: &[RGB8; N],
    ) -> Result<(), esp_hal::spi::Error> {
        assert!(B >= 12 * N + WS2812_RESET_BYTES);

        encode_sequence(self.buffer, pixels);

        self.spi.write_async(self.buffer).await?;

        Ok(())
    }
}

// ----------------------------------------------------------------

fn slice_to_array_mut<const N: usize>(s: &mut [u8]) -> &mut [u8; N] {
    assert!(s.len() >= N);
    unsafe { &mut *(s.as_mut_ptr() as *mut [u8; N]) }
}

fn encode_reset(buffer: &mut [u8; WS2812_RESET_BYTES]) {
    for i in 0..WS2812_RESET_BYTES {
        buffer[i] = 0;
    }
}

fn encode_byte(buffer: &mut [u8; 4], mut data: u8) {
    let mut index = 0;
    // Send two bits in one spi byte. High time first, then the low time
    // The maximum for T0H is 500ns, the minimum for one bit 1063 ns.
    // These result in the upper and lower spi frequency limits
    let patterns = [0b1000_1000, 0b1000_1110, 0b11101000, 0b11101110];
    for _ in 0..4 {
        let bits = (data & 0b1100_0000) >> 6;
        buffer[index] = patterns[bits as usize];
        index += 1;
        data <<= 2;
    }
}

fn encode_pixel(buffer: &mut [u8; 12], pixel: &RGB8) {
    encode_byte(slice_to_array_mut(&mut buffer[..4]), pixel.g);
    encode_byte(slice_to_array_mut(&mut buffer[4..8]), pixel.r);
    encode_byte(slice_to_array_mut(&mut buffer[8..12]), pixel.b);
}

pub fn encode_sequence<const N: usize, const B: usize>(buffer: &mut [u8; B], pixels: &[RGB8; N]) {
    assert!(B >= 12 * N + WS2812_RESET_BYTES);

    let mut index = 0;

    for pixel in pixels {
        let chunk = slice_to_array_mut::<12>(&mut buffer[index..index + 12]);
        encode_pixel(chunk, pixel);
        index += 12;
    }
    let reset_slice =
        slice_to_array_mut::<WS2812_RESET_BYTES>(&mut buffer[index..index + WS2812_RESET_BYTES]);
    encode_reset(reset_slice);
}
