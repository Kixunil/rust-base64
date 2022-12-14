//! Provides the [Engine] abstraction and out of the box implementations.
#[cfg(any(feature = "alloc", feature = "std", test))]
use crate::chunked_encoder;
use crate::{alphabet, encode::encode_with_padding, encoded_len, DecodeError};

#[cfg(any(feature = "alloc", feature = "std", test))]
use alloc::{string::String, vec};

pub mod general_purpose;

#[cfg(test)]
mod naive;

#[cfg(test)]
mod tests;

pub use general_purpose::{GeneralPurpose, GeneralPurposeConfig};

/// An `Engine` provides low-level encoding and decoding operations that all other higher-level parts of the API use. Users of the library will generally not need to implement this.
///
/// Different implementations offer different characteristics. The library currently ships with
/// a general-purpose [GeneralPurpose] impl that offers good speed and works on any CPU, with more choices
/// coming later, like a constant-time one when side channel resistance is called for, and vendor-specific vectorized ones for more speed.
///
/// See [DEFAULT_ENGINE] if you just want standard base64. Otherwise, when possible, it's
/// recommended to store the engine in a `const` so that references to it won't pose any lifetime
/// issues, and to avoid repeating the cost of engine setup.
// When adding an implementation of Engine, include them in the engine test suite:
// - add an implementation of [engine::tests::EngineWrapper]
// - add the implementation to the `all_engines` macro
// All tests run on all engines listed in the macro.
pub trait Engine: Send + Sync {
    /// The config type used by this engine
    type Config: Config;
    /// The decode estimate used by this engine
    type DecodeEstimate: DecodeEstimate;

    /// This is not meant to be called directly. See the other `encode*` functions on this trait.
    ///
    /// Encode the `input` bytes into the `output` buffer based on the mapping in `encode_table`.
    ///
    /// `output` will be long enough to hold the encoded data.
    ///
    /// Returns the number of bytes written.
    ///
    /// No padding should be written; that is handled separately.
    ///
    /// Must not write any bytes into the output slice other than the encoded data.
    fn inner_encode(&self, input: &[u8], output: &mut [u8]) -> usize;

    /// As an optimization to prevent the decoded length from being calculated twice, it is
    /// sometimes helpful to have a conservative estimate of the decoded size before doing the
    /// decoding, so this calculation is done separately and passed to [Engine::decode()] as needed.
    fn decoded_length_estimate(&self, input_len: usize) -> Self::DecodeEstimate;

    /// Decode `input` base64 bytes into the `output` buffer.
    ///
    /// `decode_estimate` is the result of [Engine::decoded_length_estimate()], which is passed in to avoid
    /// calculating it again (expensive on short inputs).`
    ///
    /// Returns the number of bytes written to `output`.
    ///
    /// Each complete 4-byte chunk of encoded data decodes to 3 bytes of decoded data, but this
    /// function must also handle the final possibly partial chunk.
    /// If the input length is not a multiple of 4, or uses padding bytes to reach a multiple of 4,
    /// the trailing 2 or 3 bytes must decode to 1 or 2 bytes, respectively, as per the
    /// [RFC](https://tools.ietf.org/html/rfc4648#section-3.5).
    ///
    /// Decoding must not write any bytes into the output slice other than the decoded data.
    ///
    /// Non-canonical trailing bits in the final tokens or non-canonical padding must be reported as
    /// errors unless the engine is configured otherwise.
    fn decode(
        &self,
        input: &[u8],
        output: &mut [u8],
        decode_estimate: Self::DecodeEstimate,
    ) -> Result<usize, DecodeError>;

    /// Returns the config for this engine.
    fn config(&self) -> &Self::Config;

    ///Encode arbitrary octets as base64 using the provided `Engine`.
    ///Returns a `String`.
    ///
    ///# Example
    ///
    ///```rust
    ///use base64::Engine as _;
    ///const URL_SAFE_ENGINE: base64::engine::GeneralPurpose =
    ///    base64::engine::GeneralPurpose::new(
    ///        &base64::alphabet::URL_SAFE,
    ///        base64::engine::general_purpose::NO_PAD);
    ///
    ///    let b64 = base64::engine::DEFAULT_ENGINE.encode(b"hello world~");
    ///    println!("{}", b64);
    ///
    ///    //////let b64_url = URL_SAFE_ENGINE.encode(b"hello internet~");
    #[cfg(any(feature = "alloc", feature = "std", test))]
    fn encode<T: AsRef<[u8]>>(&self, input: T) -> String {
        let encoded_size = encoded_len(input.as_ref().len(), self.config().encode_padding())
            .expect("integer overflow when calculating buffer size");
        let mut buf = vec![0; encoded_size];

        encode_with_padding(input.as_ref(), &mut buf[..], self, encoded_size);

        String::from_utf8(buf).expect("Invalid UTF8")
    }

    ///Encode arbitrary octets as base64 into a supplied `String`.
    ///Writes into the supplied `String`, which may allocate if its internal buffer isn't big enough.
    ///
    ///# Example
    ///
    ///```rust
    ///use base64::Engine as _;
    ///const URL_SAFE_ENGINE: base64::engine::GeneralPurpose =
    ///    base64::engine::GeneralPurpose::new(
    ///        &base64::alphabet::URL_SAFE,
    ///        base64::engine::general_purpose::NO_PAD);
    ///fn main() {
    ///    let mut buf = String::new();
    ///    base64::engine::DEFAULT_ENGINE.encode_string(b"hello world~", &mut buf);
    ///    println!("{}", buf);
    ///
    ///    buf.clear();
    ///    URL_SAFE_ENGINE.encode_string(b"hello internet~", &mut buf);
    ///    println!("{}", buf);
    ///}
    ///```
    #[cfg(any(feature = "alloc", feature = "std", test))]
    fn encode_string<T: AsRef<[u8]>>(&self, input: T, output_buf: &mut String) {
        let input_bytes = input.as_ref();

        {
            let mut sink = chunked_encoder::StringSink::new(output_buf);

            chunked_encoder::ChunkedEncoder::new(self)
                .encode(input_bytes, &mut sink)
                .expect("Writing to a String shouldn't fail");
        }
    }

    /// Encode arbitrary octets as base64 into a supplied slice.
    /// Writes into the supplied output buffer.
    ///
    /// This is useful if you wish to avoid allocation entirely (e.g. encoding into a stack-resident
    /// or statically-allocated buffer).
    ///
    /// # Panics
    ///
    /// If `output` is too small to hold the encoded version of `input`, a panic will result.
    ///
    /// # Example
    ///
    /// ```rust
    /// use base64::Engine as _;
    /// let s = b"hello internet!";
    /// let mut buf = Vec::new();
    /// // make sure we'll have a slice big enough for base64 + padding
    /// buf.resize(s.len() * 4 / 3 + 4, 0);
    ///
    /// let bytes_written = base64::engine::DEFAULT_ENGINE.encode_slice(s, &mut buf);
    ///
    /// // shorten our vec down to just what was written
    /// buf.truncate(bytes_written);
    ///
    /// assert_eq!(s, base64::decode(&buf).unwrap().as_slice());
    /// ```
    fn encode_slice<T: AsRef<[u8]>>(&self, input: T, output_buf: &mut [u8]) -> usize {
        let input_bytes = input.as_ref();

        let encoded_size = encoded_len(input_bytes.len(), self.config().encode_padding())
            .expect("usize overflow when calculating buffer size");

        let b64_output = &mut output_buf[0..encoded_size];

        encode_with_padding(input_bytes, b64_output, self, encoded_size);

        encoded_size
    }
}

/// The minimal level of configuration that engines must support.
pub trait Config {
    /// Returns `true` if padding should be added after the encoded output.
    ///
    /// Padding is added outside the engine's encode() since the engine may be used
    /// to encode only a chunk of the overall output, so it can't always know when
    /// the output is "done" and would therefore need padding (if configured).
    // It could be provided as a separate parameter when encoding, but that feels like
    // leaking an implementation detail to the user, and it's hopefully more convenient
    // to have to only pass one thing (the engine) to any part of the API.
    fn encode_padding(&self) -> bool;
}

/// The decode estimate used by an engine implementation. Users do not need to interact with this;
/// it is only for engine implementors.
///
/// Implementors may store relevant data here when constructing this to avoid having to calculate
/// them again during actual decoding.
pub trait DecodeEstimate {
    /// Returns a conservative (err on the side of too big) estimate of the decoded length to use
    /// for pre-allocating buffers, etc.
    fn decoded_length_estimate(&self) -> usize;
}

/// A [GeneralPurpose] engine using the [crate::alphabet::STANDARD] base64 alphabet and [crate::engine::general_purpose::PAD] config.
pub const DEFAULT_ENGINE: GeneralPurpose =
    GeneralPurpose::new(&alphabet::STANDARD, general_purpose::PAD);

/// Controls how pad bytes are handled when decoding.
///
/// Each [Engine] must support at least the behavior indicated by
/// [DecodePaddingMode::RequireCanonical], and may support other modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodePaddingMode {
    /// Canonical padding is allowed, but any fewer padding bytes than that is also allowed.
    Indifferent,
    /// Padding must be canonical (0, 1, or 2 `=` as needed to produce a 4 byte suffix).
    RequireCanonical,
    /// Padding must be absent -- for when you want predictable padding, without any wasted bytes.
    RequireNone,
}
