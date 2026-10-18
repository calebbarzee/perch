//! Word lists for generated names and texts. All names are invented.

pub const BRANDS: &[&str] = &[
    "Alder", "Birchwood", "Cobalt", "Dunmore", "Ember", "Fjord", "Granite", "Halcyon", "Ironleaf", "Juniper",
    "Kestrel", "Lumen", "Meridian", "Northvale", "Orchid", "Pinecrest", "Quarry", "Redstone", "Saltmarsh",
    "Tidewater", "Umber", "Vantage", "Willow", "Yarrow", "Zephyr",
];

/// `(category, item types, base price in EUR cents low, high)`.
pub const CATEGORIES: &[(&str, &[&str], u64, u64)] = &[
    ("kitchen/kettles", &["Kettle", "Electric Kettle", "Gooseneck Kettle"], 2_500, 9_000),
    ("kitchen/cookware", &["Frying Pan", "Saucepan", "Dutch Oven", "Wok"], 2_000, 15_000),
    ("electronics/headphones", &["Wireless Headphones", "Earbuds", "Studio Headphones"], 3_000, 35_000),
    ("electronics/chargers", &["USB-C Charger", "Power Bank", "Wireless Charger"], 1_500, 8_000),
    ("home/lighting", &["Desk Lamp", "Floor Lamp", "LED Bulb"], 800, 20_000),
    ("home/vacuums", &["Stick Vacuum", "Robot Vacuum", "Handheld Vacuum"], 6_000, 60_000),
    ("outdoor/tents", &["2-Person Tent", "4-Person Tent", "Ultralight Tent"], 8_000, 60_000),
    ("outdoor/backpacks", &["Daypack", "Hiking Backpack", "Travel Pack"], 3_000, 25_000),
    ("toys/puzzles", &["Jigsaw Puzzle", "Puzzle Cube", "Wooden Puzzle"], 800, 5_000),
    ("sports/cycling", &["Bike Helmet", "Bike Light", "Bike Lock"], 1_500, 15_000),
    ("office/furniture", &["Office Chair", "Standing Desk", "Monitor Arm"], 5_000, 80_000),
];

pub const COLORS: &[&str] = &["Black", "White", "Grey", "Blue", "Green", "Red", "Sand", "Olive"];

pub const SHOP_ADJ: &[&str] = &[
    "Bright", "Corner", "Harbor", "Summit", "Market", "Union", "Atlas", "Beacon", "Canal", "Delta", "Elm", "Forge",
];
pub const SHOP_NOUN: &[&str] = &["Goods", "Supply", "Outfitters", "Store", "Emporium", "Trading", "Depot", "House"];

pub const FIRST_NAMES: &[&str] = &[
    "Ana", "Ben", "Chen", "Dara", "Emil", "Fatima", "Goran", "Hana", "Ines", "Jonas", "Kofi", "Lena", "Mateo", "Nadia",
    "Oskar", "Priya", "Quinn", "Rosa", "Sami", "Tomas", "Uma", "Viktor", "Wen", "Yara", "Zoltan",
];

pub const COUNTRIES: &[&str] = &["DE", "FR", "NL", "ES", "IT", "GB", "US", "PL", "SE", "IE"];

/// Review snippets by rating (index 0 = 1 star).
pub const REVIEW_TEXT: [&[&str]; 5] = [
    &["Stopped working after a week.", "Not as described.", "Arrived damaged and support did not reply.", "Poor build quality."],
    &["Below expectations.", "Works, but feels cheap.", "Had to return the first one.", "Noisy and slow."],
    &["Does the job.", "Average for the price.", "Some good points, some bad.", "Fine, nothing special."],
    &["Good value.", "Works well, minor issues.", "Solid and well made.", "Happy with it so far."],
    &["Excellent, would buy again.", "Exactly as described.", "Best one I have owned.", "Great quality and fast delivery."],
];

pub const REVIEW_TITLES: [&str; 5] = ["Disappointed", "Not great", "Okay", "Good", "Excellent"];

pub const FLAG_REASONS: &[&str] = &["fraud", "counterfeit", "misleading", "spam", "other"];

/// Lowercase ASCII slug: letters and digits kept, runs of others become `-`.
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}
