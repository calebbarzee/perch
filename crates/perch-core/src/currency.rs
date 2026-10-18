//! ISO 4217 currency table (ARCHITECTURE 4.5, `Price`).
//!
//! Source: the [ISO 4217](https://www.iso.org/iso-4217-currency-codes.html)
//! list of active codes as published by SIX (the ISO 4217 maintenance agency),
//! taken from the `datasets/currency-codes` mirror
//! (<https://github.com/datasets/currency-codes>, file `data/codes-all.csv`,
//! retrieved 2026-10-18). Codes with a withdrawal date are excluded; for
//! example `BGN` (withdrawn 2026-01) and `ANG` (withdrawn 2025-03).
//!
//! `minor_units` is the number of decimal places of the minor unit, or `None`
//! where ISO 4217 lists "N.A." (precious metals, SDR, test and "no currency"
//! codes). A `Price` must use a code with defined minor units, because its
//! amount is an integer count of minor units.

use serde::Serialize;

/// One active ISO 4217 currency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Currency {
    /// Three-letter alphabetic code, for example `"EUR"`.
    pub code: &'static str,
    /// Decimal places of the minor unit; `None` where ISO 4217 lists N.A.
    pub minor_units: Option<u8>,
    /// English name from the ISO 4217 list.
    pub name: &'static str,
}

/// All active ISO 4217 currencies, sorted by code.
pub static CURRENCIES: &[Currency] = &[
    Currency { code: "AED", minor_units: Some(2), name: "UAE Dirham" },
    Currency { code: "AFN", minor_units: Some(2), name: "Afghani" },
    Currency { code: "ALL", minor_units: Some(2), name: "Lek" },
    Currency { code: "AMD", minor_units: Some(2), name: "Armenian Dram" },
    Currency { code: "AOA", minor_units: Some(2), name: "Kwanza" },
    Currency { code: "ARS", minor_units: Some(2), name: "Argentine Peso" },
    Currency { code: "AUD", minor_units: Some(2), name: "Australian Dollar" },
    Currency { code: "AWG", minor_units: Some(2), name: "Aruban Florin" },
    Currency { code: "AZN", minor_units: Some(2), name: "Azerbaijan Manat" },
    Currency { code: "BAM", minor_units: Some(2), name: "Convertible Mark" },
    Currency { code: "BBD", minor_units: Some(2), name: "Barbados Dollar" },
    Currency { code: "BDT", minor_units: Some(2), name: "Taka" },
    Currency { code: "BHD", minor_units: Some(3), name: "Bahraini Dinar" },
    Currency { code: "BIF", minor_units: Some(0), name: "Burundi Franc" },
    Currency { code: "BMD", minor_units: Some(2), name: "Bermudian Dollar" },
    Currency { code: "BND", minor_units: Some(2), name: "Brunei Dollar" },
    Currency { code: "BOB", minor_units: Some(2), name: "Boliviano" },
    Currency { code: "BOV", minor_units: Some(2), name: "Mvdol" },
    Currency { code: "BRL", minor_units: Some(2), name: "Brazilian Real" },
    Currency { code: "BSD", minor_units: Some(2), name: "Bahamian Dollar" },
    Currency { code: "BTN", minor_units: Some(2), name: "Ngultrum" },
    Currency { code: "BWP", minor_units: Some(2), name: "Pula" },
    Currency { code: "BYN", minor_units: Some(2), name: "Belarusian Ruble" },
    Currency { code: "BZD", minor_units: Some(2), name: "Belize Dollar" },
    Currency { code: "CAD", minor_units: Some(2), name: "Canadian Dollar" },
    Currency { code: "CDF", minor_units: Some(2), name: "Congolese Franc" },
    Currency { code: "CHE", minor_units: Some(2), name: "WIR Euro" },
    Currency { code: "CHF", minor_units: Some(2), name: "Swiss Franc" },
    Currency { code: "CHW", minor_units: Some(2), name: "WIR Franc" },
    Currency { code: "CLF", minor_units: Some(4), name: "Unidad de Fomento" },
    Currency { code: "CLP", minor_units: Some(0), name: "Chilean Peso" },
    Currency { code: "CNY", minor_units: Some(2), name: "Yuan Renminbi" },
    Currency { code: "COP", minor_units: Some(2), name: "Colombian Peso" },
    Currency { code: "COU", minor_units: Some(2), name: "Unidad de Valor Real" },
    Currency { code: "CRC", minor_units: Some(2), name: "Costa Rican Colon" },
    Currency { code: "CUP", minor_units: Some(2), name: "Cuban Peso" },
    Currency { code: "CVE", minor_units: Some(2), name: "Cabo Verde Escudo" },
    Currency { code: "CZK", minor_units: Some(2), name: "Czech Koruna" },
    Currency { code: "DJF", minor_units: Some(0), name: "Djibouti Franc" },
    Currency { code: "DKK", minor_units: Some(2), name: "Danish Krone" },
    Currency { code: "DOP", minor_units: Some(2), name: "Dominican Peso" },
    Currency { code: "DZD", minor_units: Some(2), name: "Algerian Dinar" },
    Currency { code: "EGP", minor_units: Some(2), name: "Egyptian Pound" },
    Currency { code: "ERN", minor_units: Some(2), name: "Nakfa" },
    Currency { code: "ETB", minor_units: Some(2), name: "Ethiopian Birr" },
    Currency { code: "EUR", minor_units: Some(2), name: "Euro" },
    Currency { code: "FJD", minor_units: Some(2), name: "Fiji Dollar" },
    Currency { code: "FKP", minor_units: Some(2), name: "Falkland Islands Pound" },
    Currency { code: "GBP", minor_units: Some(2), name: "Pound Sterling" },
    Currency { code: "GEL", minor_units: Some(2), name: "Lari" },
    Currency { code: "GHS", minor_units: Some(2), name: "Ghana Cedi" },
    Currency { code: "GIP", minor_units: Some(2), name: "Gibraltar Pound" },
    Currency { code: "GMD", minor_units: Some(2), name: "Dalasi" },
    Currency { code: "GNF", minor_units: Some(0), name: "Guinean Franc" },
    Currency { code: "GTQ", minor_units: Some(2), name: "Quetzal" },
    Currency { code: "GYD", minor_units: Some(2), name: "Guyana Dollar" },
    Currency { code: "HKD", minor_units: Some(2), name: "Hong Kong Dollar" },
    Currency { code: "HNL", minor_units: Some(2), name: "Lempira" },
    Currency { code: "HTG", minor_units: Some(2), name: "Gourde" },
    Currency { code: "HUF", minor_units: Some(2), name: "Forint" },
    Currency { code: "IDR", minor_units: Some(2), name: "Rupiah" },
    Currency { code: "ILS", minor_units: Some(2), name: "New Israeli Sheqel" },
    Currency { code: "INR", minor_units: Some(2), name: "Indian Rupee" },
    Currency { code: "IQD", minor_units: Some(3), name: "Iraqi Dinar" },
    Currency { code: "IRR", minor_units: Some(2), name: "Iranian Rial" },
    Currency { code: "ISK", minor_units: Some(0), name: "Iceland Krona" },
    Currency { code: "JMD", minor_units: Some(2), name: "Jamaican Dollar" },
    Currency { code: "JOD", minor_units: Some(3), name: "Jordanian Dinar" },
    Currency { code: "JPY", minor_units: Some(0), name: "Yen" },
    Currency { code: "KES", minor_units: Some(2), name: "Kenyan Shilling" },
    Currency { code: "KGS", minor_units: Some(2), name: "Som" },
    Currency { code: "KHR", minor_units: Some(2), name: "Riel" },
    Currency { code: "KMF", minor_units: Some(0), name: "Comorian Franc" },
    Currency { code: "KPW", minor_units: Some(2), name: "North Korean Won" },
    Currency { code: "KRW", minor_units: Some(0), name: "Won" },
    Currency { code: "KWD", minor_units: Some(3), name: "Kuwaiti Dinar" },
    Currency { code: "KYD", minor_units: Some(2), name: "Cayman Islands Dollar" },
    Currency { code: "KZT", minor_units: Some(2), name: "Tenge" },
    Currency { code: "LAK", minor_units: Some(2), name: "Lao Kip" },
    Currency { code: "LBP", minor_units: Some(2), name: "Lebanese Pound" },
    Currency { code: "LKR", minor_units: Some(2), name: "Sri Lanka Rupee" },
    Currency { code: "LRD", minor_units: Some(2), name: "Liberian Dollar" },
    Currency { code: "LSL", minor_units: Some(2), name: "Loti" },
    Currency { code: "LYD", minor_units: Some(3), name: "Libyan Dinar" },
    Currency { code: "MAD", minor_units: Some(2), name: "Moroccan Dirham" },
    Currency { code: "MDL", minor_units: Some(2), name: "Moldovan Leu" },
    Currency { code: "MGA", minor_units: Some(2), name: "Malagasy Ariary" },
    Currency { code: "MKD", minor_units: Some(2), name: "Denar" },
    Currency { code: "MMK", minor_units: Some(2), name: "Kyat" },
    Currency { code: "MNT", minor_units: Some(2), name: "Tugrik" },
    Currency { code: "MOP", minor_units: Some(2), name: "Pataca" },
    Currency { code: "MRU", minor_units: Some(2), name: "Ouguiya" },
    Currency { code: "MUR", minor_units: Some(2), name: "Mauritius Rupee" },
    Currency { code: "MVR", minor_units: Some(2), name: "Rufiyaa" },
    Currency { code: "MWK", minor_units: Some(2), name: "Malawi Kwacha" },
    Currency { code: "MXN", minor_units: Some(2), name: "Mexican Peso" },
    Currency { code: "MXV", minor_units: Some(2), name: "Mexican Unidad de Inversion (UDI)" },
    Currency { code: "MYR", minor_units: Some(2), name: "Malaysian Ringgit" },
    Currency { code: "MZN", minor_units: Some(2), name: "Mozambique Metical" },
    Currency { code: "NAD", minor_units: Some(2), name: "Namibia Dollar" },
    Currency { code: "NGN", minor_units: Some(2), name: "Naira" },
    Currency { code: "NIO", minor_units: Some(2), name: "Cordoba Oro" },
    Currency { code: "NOK", minor_units: Some(2), name: "Norwegian Krone" },
    Currency { code: "NPR", minor_units: Some(2), name: "Nepalese Rupee" },
    Currency { code: "NZD", minor_units: Some(2), name: "New Zealand Dollar" },
    Currency { code: "OMR", minor_units: Some(3), name: "Rial Omani" },
    Currency { code: "PAB", minor_units: Some(2), name: "Balboa" },
    Currency { code: "PEN", minor_units: Some(2), name: "Sol" },
    Currency { code: "PGK", minor_units: Some(2), name: "Kina" },
    Currency { code: "PHP", minor_units: Some(2), name: "Philippine Peso" },
    Currency { code: "PKR", minor_units: Some(2), name: "Pakistan Rupee" },
    Currency { code: "PLN", minor_units: Some(2), name: "Zloty" },
    Currency { code: "PYG", minor_units: Some(0), name: "Guarani" },
    Currency { code: "QAR", minor_units: Some(2), name: "Qatari Rial" },
    Currency { code: "RON", minor_units: Some(2), name: "Romanian Leu" },
    Currency { code: "RSD", minor_units: Some(2), name: "Serbian Dinar" },
    Currency { code: "RUB", minor_units: Some(2), name: "Russian Ruble" },
    Currency { code: "RWF", minor_units: Some(0), name: "Rwanda Franc" },
    Currency { code: "SAR", minor_units: Some(2), name: "Saudi Riyal" },
    Currency { code: "SBD", minor_units: Some(2), name: "Solomon Islands Dollar" },
    Currency { code: "SCR", minor_units: Some(2), name: "Seychelles Rupee" },
    Currency { code: "SDG", minor_units: Some(2), name: "Sudanese Pound" },
    Currency { code: "SEK", minor_units: Some(2), name: "Swedish Krona" },
    Currency { code: "SGD", minor_units: Some(2), name: "Singapore Dollar" },
    Currency { code: "SHP", minor_units: Some(2), name: "Saint Helena Pound" },
    Currency { code: "SLE", minor_units: Some(2), name: "Leone" },
    Currency { code: "SOS", minor_units: Some(2), name: "Somali Shilling" },
    Currency { code: "SRD", minor_units: Some(2), name: "Surinam Dollar" },
    Currency { code: "SSP", minor_units: Some(2), name: "South Sudanese Pound" },
    Currency { code: "STN", minor_units: Some(2), name: "Dobra" },
    Currency { code: "SVC", minor_units: Some(2), name: "El Salvador Colon" },
    Currency { code: "SYP", minor_units: Some(2), name: "Syrian Pound" },
    Currency { code: "SZL", minor_units: Some(2), name: "Lilangeni" },
    Currency { code: "THB", minor_units: Some(2), name: "Baht" },
    Currency { code: "TJS", minor_units: Some(2), name: "Somoni" },
    Currency { code: "TMT", minor_units: Some(2), name: "Turkmenistan New Manat" },
    Currency { code: "TND", minor_units: Some(3), name: "Tunisian Dinar" },
    Currency { code: "TOP", minor_units: Some(2), name: "Pa’anga" },
    Currency { code: "TRY", minor_units: Some(2), name: "Turkish Lira" },
    Currency { code: "TTD", minor_units: Some(2), name: "Trinidad and Tobago Dollar" },
    Currency { code: "TWD", minor_units: Some(2), name: "New Taiwan Dollar" },
    Currency { code: "TZS", minor_units: Some(2), name: "Tanzanian Shilling" },
    Currency { code: "UAH", minor_units: Some(2), name: "Hryvnia" },
    Currency { code: "UGX", minor_units: Some(0), name: "Uganda Shilling" },
    Currency { code: "USD", minor_units: Some(2), name: "US Dollar" },
    Currency { code: "USN", minor_units: Some(2), name: "US Dollar (Next day)" },
    Currency { code: "UYI", minor_units: Some(0), name: "Uruguay Peso en Unidades Indexadas (UI)" },
    Currency { code: "UYU", minor_units: Some(2), name: "Peso Uruguayo" },
    Currency { code: "UYW", minor_units: Some(4), name: "Unidad Previsional" },
    Currency { code: "UZS", minor_units: Some(2), name: "Uzbekistan Sum" },
    Currency { code: "VED", minor_units: Some(2), name: "Bolívar Soberano" },
    Currency { code: "VES", minor_units: Some(2), name: "Bolívar Soberano" },
    Currency { code: "VND", minor_units: Some(0), name: "Dong" },
    Currency { code: "VUV", minor_units: Some(0), name: "Vatu" },
    Currency { code: "WST", minor_units: Some(2), name: "Tala" },
    Currency { code: "XAD", minor_units: Some(2), name: "Arab Accounting Dinar" },
    Currency { code: "XAF", minor_units: Some(0), name: "CFA Franc BEAC" },
    Currency { code: "XAG", minor_units: None, name: "Silver" },
    Currency { code: "XAU", minor_units: None, name: "Gold" },
    Currency { code: "XBA", minor_units: None, name: "Bond Markets Unit European Composite Unit (EURCO)" },
    Currency { code: "XBB", minor_units: None, name: "Bond Markets Unit European Monetary Unit (E.M.U.-6)" },
    Currency { code: "XBC", minor_units: None, name: "Bond Markets Unit European Unit of Account 9 (E.U.A.-9)" },
    Currency { code: "XBD", minor_units: None, name: "Bond Markets Unit European Unit of Account 17 (E.U.A.-17)" },
    Currency { code: "XCD", minor_units: Some(2), name: "East Caribbean Dollar" },
    Currency { code: "XCG", minor_units: Some(2), name: "Caribbean Guilder" },
    Currency { code: "XDR", minor_units: None, name: "SDR (Special Drawing Right)" },
    Currency { code: "XOF", minor_units: Some(0), name: "CFA Franc BCEAO" },
    Currency { code: "XPD", minor_units: None, name: "Palladium" },
    Currency { code: "XPF", minor_units: Some(0), name: "CFP Franc" },
    Currency { code: "XPT", minor_units: None, name: "Platinum" },
    Currency { code: "XSU", minor_units: None, name: "Sucre" },
    Currency { code: "XTS", minor_units: None, name: "Codes specifically reserved for testing purposes" },
    Currency { code: "XUA", minor_units: None, name: "ADB Unit of Account" },
    Currency { code: "XXX", minor_units: None, name: "The codes assigned for transactions where no currency is involved" },
    Currency { code: "YER", minor_units: Some(2), name: "Yemeni Rial" },
    Currency { code: "ZAR", minor_units: Some(2), name: "Rand" },
    Currency { code: "ZMW", minor_units: Some(2), name: "Zambian Kwacha" },
    Currency { code: "ZWG", minor_units: Some(2), name: "Zimbabwe Gold" },
];

/// Looks up an active currency by its alphabetic code (case-sensitive).
pub fn currency(code: &str) -> Option<&'static Currency> {
    CURRENCIES.binary_search_by(|c| c.code.cmp(code)).ok().map(|i| &CURRENCIES[i])
}

/// Number of minor-unit decimal places for `code`, if the code is active and
/// has defined minor units.
pub fn minor_units(code: &str) -> Option<u8> {
    currency(code).and_then(|c| c.minor_units)
}

/// True if `code` is valid for `Price.currency`: an active ISO 4217 code
/// with defined minor units.
pub fn is_price_currency(code: &str) -> bool {
    minor_units(code).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_sorted_and_unique() {
        assert!(CURRENCIES.windows(2).all(|w| w[0].code < w[1].code));
        assert!(CURRENCIES.iter().all(|c| c.code.len() == 3 && c.code.bytes().all(|b| b.is_ascii_uppercase())));
    }

    #[test]
    fn known_values() {
        assert_eq!(minor_units("EUR"), Some(2));
        assert_eq!(minor_units("USD"), Some(2));
        assert_eq!(minor_units("JPY"), Some(0));
        assert_eq!(minor_units("KWD"), Some(3));
        assert_eq!(minor_units("CLF"), Some(4));
        assert_eq!(minor_units("XAU"), None);
        assert!(currency("XAU").is_some());
        assert!(!is_price_currency("XAU"));
        assert!(!is_price_currency("XXX"));
        assert!(!is_price_currency("eur"));
        assert!(!is_price_currency("BGN"));
        assert!(!is_price_currency("ABC"));
        assert!(is_price_currency("XCG"));
    }
}
