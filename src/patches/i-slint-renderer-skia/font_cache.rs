// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

use clru::CLruCache;
use i_slint_common::sharedfontique::HashedBlob;
use i_slint_core::textlayout::sharedparley::{fontique, parley};
use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;

const FONT_CACHE_CAPACITY: NonZeroUsize = NonZeroUsize::new(64).unwrap();

pub struct FontCache {
    font_mgr: skia_safe::FontMgr,
    // Use HashedBlob in key to keep strong reference to font data blob,
    // preventing eviction from fontique's shared cache (see commit 30a03cf).
    // The u64 is a hash of variation settings (0 for base typefaces).
    fonts: CLruCache<(HashedBlob, u32, u64), Option<skia_safe::Typeface>>,
}

impl Default for FontCache {
    fn default() -> Self {
        Self { font_mgr: skia_safe::FontMgr::new(), fonts: CLruCache::new(FONT_CACHE_CAPACITY) }
    }
}

impl FontCache {
    pub fn font_with_variations(
        &mut self,
        font: &parley::FontData,
        synthesis: &fontique::Synthesis,
    ) -> Option<skia_safe::Typeface> {
        let variation_settings = synthesis.variation_settings();

        let mut variations_hash = 0u64;
        if !variation_settings.is_empty() {
            let mut hasher = DefaultHasher::new();
            for &(tag, value) in variation_settings {
                tag.to_be_bytes().hash(&mut hasher);
                value.to_bits().hash(&mut hasher);
            }
            variations_hash = hasher.finish();
        }

        let key = (font.data.clone().into(), font.index, variations_hash);

        if let Some(cached) = self.fonts.get(&key) {
            return cached.clone();
        }

        let mut typeface = self.load_typeface_internal(font);

        if !variation_settings.is_empty() {
            typeface = typeface.and_then(|base| {
                let coords: Vec<skia_safe::font_arguments::variation_position::Coordinate> =
                    variation_settings
                        .iter()
                        .map(|&(tag, value)| {
                            skia_safe::font_arguments::variation_position::Coordinate {
                                axis: skia_safe::FourByteTag::new(u32::from_be_bytes(
                                    tag.to_be_bytes(),
                                )),
                                value,
                            }
                        })
                        .collect();
                let position =
                    skia_safe::font_arguments::VariationPosition { coordinates: &coords };
                let args = skia_safe::FontArguments::new().set_variation_design_position(position);
                base.clone_with_arguments(&args).or(Some(base))
            });
        }

        self.fonts.put(key, typeface.clone());
        typeface
    }

    fn load_typeface_internal(&self, font: &parley::FontData) -> Option<skia_safe::Typeface> {
        let typeface = self.font_mgr.new_from_data(
            skia_safe::Data::new_copy(font.data.as_ref()),
            if font.index > 0 { Some(font.index as _) } else { None },
        );

        // Due to  https://issues.skia.org/issues/310510989, fonts from true type collections
        // with an index > 0 fail to load on macOS. As a workaround, we manually extract the font from the
        // collection and load it as a single font.
        #[cfg(target_vendor = "apple")]
        if font.index > 0
            && typeface.is_none()
            && let Some(typeface) = read_fonts::CollectionRef::new(font.data.as_ref())
                .ok()
                .and_then(|ttc| ttc.get(font.index).ok())
                .map(|ttf| write_fonts::FontBuilder::new().copy_missing_tables(ttf).build())
                .and_then(|new_ttf| {
                    self.font_mgr.new_from_data(skia_safe::Data::new_copy(&new_ttf), None)
                })
        {
            return Some(typeface);
        }

        typeface
    }
}

use skrifa::{
    FontRef, GlyphId, MetadataProvider,
    instance::{LocationRef, NormalizedCoord, Size},
    outline::{
        DrawSettings, Engine, HintingInstance, HintingOptions, OutlinePen, SmoothMode, Target,
    },
};

const HINTER_CACHE_CAPACITY: NonZeroUsize = NonZeroUsize::new(32).unwrap();
const GLYPH_CACHE_CAPACITY: NonZeroUsize = NonZeroUsize::new(4096).unwrap();

#[derive(Clone)]
pub enum CachedGlyph {
    Path(skia_safe::Path),
    Blank,
    ColorOrBitmapFallback,
}

struct SkiaOutlinePen<'a> {
    builder: &'a mut skia_safe::PathBuilder,
    shear: f32,
}

impl OutlinePen for SkiaOutlinePen<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        self.builder.move_to((x + y * self.shear, -y));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.builder.line_to((x + y * self.shear, -y));
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.builder.quad_to(
            (cx0 + cy0 * self.shear, -cy0),
            (x + y * self.shear, -y),
        );
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.builder.cubic_to(
            (cx0 + cy0 * self.shear, -cy0),
            (cx1 + cy1 * self.shear, -cy1),
            (x + y * self.shear, -y),
        );
    }

    fn close(&mut self) {
        self.builder.close();
    }
}

pub struct GlyphOutlineCache {
    hinters: CLruCache<(HashedBlob, u32, u32, u64), Option<HintingInstance>>,
    glyphs: CLruCache<(HashedBlob, u32, u32, u64, u32), CachedGlyph>,
}

impl Default for GlyphOutlineCache {
    fn default() -> Self {
        Self {
            hinters: CLruCache::new(HINTER_CACHE_CAPACITY),
            glyphs: CLruCache::new(GLYPH_CACHE_CAPACITY),
        }
    }
}

impl GlyphOutlineCache {
    pub fn style_keys(
        font_size: f32,
        normalized_coords: &[i16],
        synthesis: &fontique::Synthesis,
        hinted: bool,
    ) -> (u32, f32, u64, u64, f32) {
        let size_26_6 = (font_size.max(0.5) * 64.0).round() as u32;
        let snapped_ppem = size_26_6 as f32 / 64.0;
        let shear = synthesis.skew().map_or(0.0, |deg| deg.to_radians().tan());

        let mut coords_hasher = DefaultHasher::new();
        normalized_coords.hash(&mut coords_hasher);
        let coords_hash = coords_hasher.finish();

        let mut style_hasher = DefaultHasher::new();
        coords_hash.hash(&mut style_hasher);
        shear.to_bits().hash(&mut style_hasher);
        hinted.hash(&mut style_hasher);
        let style_hash = style_hasher.finish();

        (size_26_6, snapped_ppem, coords_hash, style_hash, shear)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn glyph_outline(
        &mut self,
        blob: &HashedBlob,
        font: &parley::FontData,
        size_26_6: u32,
        snapped_ppem: f32,
        normalized_coords: &[i16],
        coords_hash: u64,
        style_hash: u64,
        shear: f32,
        hinted: bool,
        glyph_id: u32,
    ) -> CachedGlyph {
        let glyph_key = (blob.clone(), font.index, size_26_6, style_hash, glyph_id);
        if let Some(cached) = self.glyphs.get(&glyph_key) {
            return cached.clone();
        }

        let Ok(font_ref) = FontRef::from_index(font.data.as_ref(), font.index) else {
            self.glyphs.put(glyph_key, CachedGlyph::ColorOrBitmapFallback);
            return CachedGlyph::ColorOrBitmapFallback;
        };

        let gid = GlyphId::from(glyph_id);
        if font_ref.color_glyphs().get(gid).is_some() {
            self.glyphs.put(glyph_key, CachedGlyph::ColorOrBitmapFallback);
            return CachedGlyph::ColorOrBitmapFallback;
        }

        let outlines = font_ref.outline_glyphs();
        let Some(outline_glyph) = outlines.get(gid) else {
            self.glyphs.put(glyph_key, CachedGlyph::ColorOrBitmapFallback);
            return CachedGlyph::ColorOrBitmapFallback;
        };

        let coords: Vec<NormalizedCoord> =
            normalized_coords.iter().copied().map(NormalizedCoord::from_bits).collect();
        let location = LocationRef::new(&coords);
        let size = Size::new(snapped_ppem);

        let mut builder =
            skia_safe::PathBuilder::new_with_fill_type(skia_safe::PathFillType::Winding);
        let mut pen = SkiaOutlinePen { builder: &mut builder, shear };

        let drew = if hinted {
            let hinter_key = (blob.clone(), font.index, size_26_6, coords_hash);
            let hinter_opt = if let Some(existing) = self.hinters.get(&hinter_key) {
                existing.clone()
            } else {
                let options = HintingOptions {
                    engine: Engine::AutoFallback,
                    target: Target::Smooth {
                        mode: SmoothMode::Normal,
                        symmetric_rendering: true,
                        preserve_linear_metrics: true,
                    },
                };
                let created = HintingInstance::new(&outlines, size, location, options).ok();
                self.hinters.put(hinter_key, created.clone());
                created
            };

            if let Some(ref hinter) = hinter_opt {
                outline_glyph
                    .draw(DrawSettings::hinted(hinter, false), &mut pen)
                    .or_else(|_| outline_glyph.draw(DrawSettings::unhinted(size, location), &mut pen))
                    .is_ok()
            } else {
                outline_glyph.draw(DrawSettings::unhinted(size, location), &mut pen).is_ok()
            }
        } else {
            outline_glyph.draw(DrawSettings::unhinted(size, location), &mut pen).is_ok()
        };

        let entry = if !drew || builder.is_empty() {
            CachedGlyph::Blank
        } else {
            CachedGlyph::Path(builder.detach())
        };

        self.glyphs.put(glyph_key, entry.clone());
        entry
    }
}

thread_local! {
    pub static FONT_CACHE: RefCell<FontCache> = RefCell::new(Default::default());
    pub static GLYPH_OUTLINE_CACHE: RefCell<GlyphOutlineCache> = RefCell::new(Default::default());
}
