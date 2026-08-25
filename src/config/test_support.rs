//! Fixtures for the configuration tests.

/// A real certificate, for tests that configure a root without opening a
/// connection.
///
/// `database_root_certificate` validates by parsing, so a fabricated blob does
/// not reach the assertions. Its subject and its key do not matter: nothing
/// verifies a chain against it.
pub(crate) const ROOT_CERTIFICATE: &str = concat!(
    "-----BEGIN CERTIFICATE-----\n",
    "MIIBlDCCATugAwIBAgIUPEjFyboN/ZTnaAixdCCeZ/zw3nEwCgYIKoZIzj0EAwIw\n",
    "HzEdMBsGA1UEAwwUUHJvb2ZwbGFuZSB0ZXN0IHJvb3QwIBcNMjYwODIzMDMyNTU1\n",
    "WhgPMjEyNjA3MzAwMzI1NTVaMB8xHTAbBgNVBAMMFFByb29mcGxhbmUgdGVzdCBy\n",
    "b290MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAETQlzc9Zv1ixN7xCChzLcC7LW\n",
    "iruRK/qQu/e2bRPu2D4Ejem69E+2cI9Kd5Re1DS2ydE/u4KJtPHqBMX3w/8NUKNT\n",
    "MFEwHQYDVR0OBBYEFDd5V3KAQbTv4ZDLpk5QY1Fcreu4MB8GA1UdIwQYMBaAFDd5\n",
    "V3KAQbTv4ZDLpk5QY1Fcreu4MA8GA1UdEwEB/wQFMAMBAf8wCgYIKoZIzj0EAwID\n",
    "RwAwRAIgBNMEQzl8tm1ohW9+Yh9mXSOyLbSEph/EF0iSEE4w/IkCIA7SO8PSU/IS\n",
    "xAle8khrvo3kebW6vpXfHRjisM2rI3V8\n",
    "-----END CERTIFICATE-----\n",
);
