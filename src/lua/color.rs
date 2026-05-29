//! Lightweight Color Management.
//!
//! This module provides the `icefield.color` table, offering a robust set of
//! tools for parsing, manipulating, and formatting colors directly from Lua.
//! Colors are represented as immutable `UserData` objects, allowing for
//! fluent chaining of operations like HSL shifts and alpha modifications.

use crate::lua::registry::{ApiRegistry, LuaApiItem, LuaItemKind};
use mlua::{Lua, Result, Table, UserData, UserDataFields, UserDataMethods};
use palette::{Hsla, IntoColor, Srgba};
use std::str::FromStr;

/// Registers the color manipulation functions in the `icefield.color` table.
pub fn register(
    icefield: &Table,
    lua: &Lua,
    registry: &mut ApiRegistry,
) -> Result<()> {
    let color = lua.create_table()?;

    registry.register_func(
        &color,
        lua,
        LuaApiItem {
            table: "color",
            name: "from_hex",
            description: "Creates a Color object from a HEX string (with or without '#').",
            example: Some(r##"
                local c = icefield.color.from_hex("#ff5500")
            "##),
            kind: LuaItemKind::Function {
                params: &[("s", "string")],
                returns: "icefield.Color",
            },
        },
        |_, s: String| {
            let normalized = if !s.starts_with('#') {
                format!("#{}", s)
            } else {
                s.clone()
            };

            // Try parsing as 6-digit RGB first, then 8-digit RGBA
            let srgb_u8 = if let Ok(rgb) = palette::Srgb::<u8>::from_str(&normalized) {
                Srgba::from(rgb)
            } else if let Ok(rgba) = Srgba::<u8>::from_str(&normalized) {
                rgba
            } else {
                return Err(mlua::Error::RuntimeError(format!("Invalid HEX color: {}", s)));
            };

            Ok(Color(srgb_u8.into_format()))
        },
    )?;

    registry.register_func(
        &color,
        lua,
        LuaApiItem {
            table: "color",
            name: "from_rgb",
            description: "Creates a Color object from RGB(A) values.",
            example: Some(
                r##"
                local c = icefield.color.from_rgb(255, 85, 0, 1.0)
            "##,
            ),
            kind: LuaItemKind::Function {
                params: &[
                    ("r", "number"),
                    ("g", "number"),
                    ("b", "number"),
                    ("a", "number|nil"),
                ],
                returns: "icefield.Color",
            },
        },
        |_, (r, g, b, a): (u8, u8, u8, Option<f32>)| {
            let alpha = a.unwrap_or(1.0).clamp(0.0, 1.0);
            let srgb_u8 = Srgba::new(r, g, b, (alpha * 255.0).round() as u8);
            Ok(Color(srgb_u8.into_format()))
        },
    )?;

    registry.register_func(
        &color,
        lua,
        LuaApiItem {
            table: "color",
            name: "from_hsl",
            description: "Creates a Color object from HSL(A) values.",
            example: Some(
                r##"
                local c = icefield.color.from_hsl(20, 100, 50)
            "##,
            ),
            kind: LuaItemKind::Function {
                params: &[
                    ("h", "number"),
                    ("s", "number"),
                    ("l", "number"),
                    ("a", "number|nil"),
                ],
                returns: "icefield.Color",
            },
        },
        |_, (h, s, l, a): (f32, f32, f32, Option<f32>)| {
            let alpha = a.unwrap_or(1.0).clamp(0.0, 1.0);
            let hsla = Hsla::new(
                h,
                (s / 100.0).clamp(0.0, 1.0),
                (l / 100.0).clamp(0.0, 1.0),
                alpha,
            );
            Ok(Color(hsla.into_color()))
        },
    )?;

    icefield.set("color", color)?;
    Ok(())
}

/// Represents an immutable color in the Lua environment.
///
/// Internally stored as an sRGB color with a linear alpha channel (f32).
#[derive(Clone, Copy, Debug)]
pub struct Color(
    /// The underlying `palette::Srgba` representation used for color math.
    pub Srgba<f32>,
);

impl UserData for Color {
    /// Registers the readable properties (extractors) for the Color object.
    ///
    /// Available fields:
    /// - `r`, `g`, `b`: RGB channels (0-255).
    /// - `a`: Alpha channel (0.0-1.0).
    /// - `h`: Hue channel (0.0-360.0 degrees).
    /// - `s`, `l`: Saturation and Lightness channels (0.0-100.0 percent).
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("r", |_, this| {
            Ok((this.0.red * 255.0).round() as u8)
        });
        fields.add_field_method_get("g", |_, this| {
            Ok((this.0.green * 255.0).round() as u8)
        });
        fields.add_field_method_get("b", |_, this| {
            Ok((this.0.blue * 255.0).round() as u8)
        });
        fields.add_field_method_get("a", |_, this| Ok(this.0.alpha));

        fields.add_field_method_get("h", |_, this| {
            let hsla: Hsla = this.0.into_color();
            Ok(hsla.hue.into_positive_degrees())
        });
        fields.add_field_method_get("s", |_, this| {
            let hsla: Hsla = this.0.into_color();
            Ok(hsla.saturation * 100.0)
        });
        fields.add_field_method_get("l", |_, this| {
            let hsla: Hsla = this.0.into_color();
            Ok(hsla.lightness * 100.0)
        });
    }

    /// Registers the methods for the Color object.
    ///
    /// Available methods:
    /// - `to_hex(with_hash)`: Returns the HEX string representation.
    /// - `to_rgb()`: Returns the RGB string representation.
    /// - `to_rgba()`: Returns the RGBA string representation.
    /// - `to_hsl()`: Returns the HSL string representation.
    /// - `to_hsla()`: Returns the HSLA string representation.
    /// - `shift_hsl(shifts)`: Returns a new Color with shifted HSL values.
    /// - `alpha(a)`: Returns a new Color with the specified alpha value.
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("to_hex", |_, this, with_hash: Option<bool>| {
            let srgb_u8: Srgba<u8> = this.0.into_format();
            let hash = with_hash.unwrap_or(true);
            let hex = if this.0.alpha < 1.0 {
                format!(
                    "{:02x}{:02x}{:02x}{:02x}",
                    srgb_u8.red, srgb_u8.green, srgb_u8.blue, srgb_u8.alpha
                )
            } else {
                format!(
                    "{:02x}{:02x}{:02x}",
                    srgb_u8.red, srgb_u8.green, srgb_u8.blue
                )
            };
            Ok(if hash { format!("#{}", hex) } else { hex })
        });

        methods.add_method("to_rgb", |_, this, ()| {
            let srgb_u8: Srgba<u8> = this.0.into_format();
            Ok(format!(
                "rgb({}, {}, {})",
                srgb_u8.red, srgb_u8.green, srgb_u8.blue
            ))
        });

        methods.add_method("to_rgba", |_, this, ()| {
            let srgb_u8: Srgba<u8> = this.0.into_format();
            Ok(format!(
                "rgba({}, {}, {}, {:.3})",
                srgb_u8.red, srgb_u8.green, srgb_u8.blue, this.0.alpha
            ))
        });

        methods.add_method("to_hsl", |_, this, ()| {
            let hsla: Hsla = this.0.into_color();
            Ok(format!(
                "hsl({:.0}, {:.0}%, {:.0}%)",
                hsla.hue.into_positive_degrees(),
                hsla.saturation * 100.0,
                hsla.lightness * 100.0
            ))
        });

        methods.add_method("to_hsla", |_, this, ()| {
            let hsla: Hsla = this.0.into_color();
            Ok(format!(
                "hsla({:.0}, {:.0}%, {:.0}%, {:.3})",
                hsla.hue.into_positive_degrees(),
                hsla.saturation * 100.0,
                hsla.lightness * 100.0,
                hsla.alpha
            ))
        });

        methods.add_method("shift_hsl", |_, this, shifts: Table| {
            let mut hsla: Hsla = this.0.into_color();
            if let Ok(h) = shifts.get::<f32>("h") {
                hsla.hue += h;
            }
            if let Ok(s) = shifts.get::<f32>("s") {
                hsla.saturation =
                    (hsla.saturation + s / 100.0).clamp(0.0, 1.0);
            }
            if let Ok(l) = shifts.get::<f32>("l") {
                hsla.lightness = (hsla.lightness + l / 100.0).clamp(0.0, 1.0);
            }
            Ok(Color(hsla.into_color()))
        });

        methods.add_method("alpha", |_, this, a: f32| {
            let mut c = this.0;
            c.alpha = a.clamp(0.0, 1.0);
            Ok(Color(c))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_hex_parsing() -> Result<()> {
        let lua = Lua::new();
        let mut registry = ApiRegistry::new();
        let icefield = lua.create_table()?;
        register(&icefield, &lua, &mut registry)?;
        lua.globals().set("icefield", icefield)?;

        let r: u8 = lua
            .load("return icefield.color.from_hex('#ff0000').r")
            .eval()?;
        assert_eq!(r, 255);

        let g: u8 = lua
            .load("return icefield.color.from_hex('00ff00').g")
            .eval()?;
        assert_eq!(g, 255);

        let hex: String = lua
            .load("return icefield.color.from_rgb(0, 0, 255):to_hex()")
            .eval()?;
        assert_eq!(hex, "#0000ff");

        let hex_no_hash: String = lua
            .load("return icefield.color.from_rgb(0, 0, 255):to_hex(false)")
            .eval()?;
        assert_eq!(hex_no_hash, "0000ff");

        Ok(())
    }

    #[test]
    fn test_color_hsl_shifts() -> Result<()> {
        let lua = Lua::new();
        let mut registry = ApiRegistry::new();
        let icefield = lua.create_table()?;
        register(&icefield, &lua, &mut registry)?;
        lua.globals().set("icefield", icefield)?;

        // Base: hsl(0, 100, 50) -> Red #ff0000
        // Shift: h + 120 -> hsl(120, 100, 50) -> Green #00ff00
        let hex: String = lua
            .load(
                r##"
            local c = icefield.color.from_hex("#ff0000")
            local shifted = c:shift_hsl({ h = 120 })
            return shifted:to_hex()
        "##,
            )
            .eval()?;
        assert_eq!(hex, "#00ff00");

        // Shift: l - 50 -> hsl(0, 100, 0) -> Black #000000
        let black: String = lua
            .load(
                r##"
            local c = icefield.color.from_hex("#ff0000")
            return c:shift_hsl({ l = -50 }):to_hex()
        "##,
            )
            .eval()?;
        assert_eq!(black, "#000000");

        Ok(())
    }
}
