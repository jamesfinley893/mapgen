mod fields;
mod precipitation;
mod temperature;
mod wind;

use noise::OpenSimplex;

use crate::generate::util::latitude_factor;
use crate::{World, WorldConfig};

use fields::ClimateFields;
use precipitation::PrecipitationModel;

pub(super) fn generate_climate(
    world: &mut World,
    config: &WorldConfig,
    ocean: &[bool],
    climate: &OpenSimplex,
) {
    let fields = ClimateFields::sample(world, ocean);
    let precipitation_model = PrecipitationModel::new(&fields, climate);
    let wind_tilt = wind::prevailing_wind_angle(world.seed);

    for y in 0..world.height {
        let lat = latitude_factor(y, world.height);
        let wind = wind::wind_at_latitude(wind_tilt, lat);

        for x in 0..world.width {
            let idx = world.idx(x, y);
            let temperature =
                temperature::sample_temperature(world, config, &fields, climate, x, y, lat);
            let precipitation =
                precipitation_model.sample_precipitation(world, config, wind, x, y, lat);

            let tile = &mut world.tiles[idx];
            tile.temperature = temperature;
            tile.moisture = precipitation;
            tile.precipitation = precipitation;
            tile.ocean_distance = fields.distance_to_ocean[idx];
            tile.continentality = fields.continentality[idx];
        }
    }
}
