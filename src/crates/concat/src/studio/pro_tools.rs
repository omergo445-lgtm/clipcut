// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and ClipCut contributors
//! Creator All-Rounder Pro Tools:
//! 1. Timeline Markers & YouTube Chapters (`M`, `[`, `]`)
//! 2. In / Out Loop Range & Loop Playback (`I`, `O`, `Shift+L`)
//! 3. Auto-Cut Silence (Smart Jump-Cut)
//! 4. Copy & Paste Clip Attributes (`Ctrl+Shift+C`, `Ctrl+Shift+V`)
//! 5. Ripple Close All Gaps
//! 6. Track Select Forward (`A`)
//! 7. Quick Fades & Crossfade All Cuts
//! 8. Clip Color Tags (7 colors + Default)
//! 9. Preview Safe Zones, Rule-of-Thirds & 9:16 Vertical Crop Guides
//! 10. Picture-in-Picture & Split-Screen Layouts

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use super::*;

const TOOLS_META_FILE: &str = "clipcut-tools.json";

/// One timeline marker on the ruler, with a timestamp, label, and color index.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TimelineMarker {
    pub time: f32,
    pub name: String,
    pub color: i32,
}

/// Per-timeline In / Out loop range in seconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LoopRange {
    pub in_point: Option<f32>,
    pub out_point: Option<f32>,
}

/// Sidecar project metadata stored in `<project_folder>/clipcut-tools.json`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProjectToolsMeta {
    /// Timeline markers keyed by `timeline_id`.
    pub markers: HashMap<String, Vec<TimelineMarker>>,
    /// In/Out loop ranges keyed by `timeline_id`.
    pub loop_ranges: HashMap<String, LoopRange>,
    /// Clip color tag (`1..=7`) keyed by `clip_id`.
    pub clip_tags: HashMap<String, i32>,
}

impl Studio {
    // ── persistence (`clipcut-tools.json`) ──

    pub(crate) fn load_tools_meta(&mut self) {
        self.tools_meta = self
            .session
            .as_ref()
            .map(|session| std::path::Path::new(session.path()).join(TOOLS_META_FILE))
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|json| serde_json::from_str::<ProjectToolsMeta>(&json).ok())
            .unwrap_or_default();
    }

    pub(crate) fn save_tools_meta(&self) {
        let Some(session) = self.session.as_ref() else {
            return;
        };
        let path = std::path::Path::new(session.path()).join(TOOLS_META_FILE);
        if let Ok(json) = serde_json::to_string_pretty(&self.tools_meta) {
            let _ = std::fs::write(path, json);
        }
    }

    /// Color tag (`0` = default, `1..=7` = color palette) for `clip_id`.
    pub fn clip_tag_color(&self, clip_id: &str) -> i32 {
        self.tools_meta
            .clip_tags
            .get(clip_id)
            .copied()
            .unwrap_or(0)
            .clamp(0, 7)
    }

    /// Sorted markers for the active timeline.
    pub fn active_markers(&self) -> Vec<TimelineMarker> {
        let id = &self.timeline().id;
        let mut list = self
            .tools_meta
            .markers
            .get(id)
            .cloned()
            .unwrap_or_default();
        list.sort_by(|a, b| a.time.total_cmp(&b.time));
        list
    }

    /// Raw `(in_point, out_point)` for the active timeline (`-1.0` when unset).
    pub fn active_loop_points(&self) -> (f32, f32) {
        let id = &self.timeline().id;
        let range = self.tools_meta.loop_ranges.get(id).copied().unwrap_or_default();
        (
            range.in_point.unwrap_or(-1.0),
            range.out_point.unwrap_or(-1.0),
        )
    }

    /// Valid `(in_point, out_point)` when both are set and `out_point > in_point + 0.05`.
    pub fn active_loop_range(&self) -> Option<(f32, f32)> {
        let (in_pt, out_pt) = self.active_loop_points();
        if in_pt >= 0.0 && out_pt > in_pt + 0.05 {
            Some((in_pt, out_pt))
        } else {
            None
        }
    }

    /// Dispatches any of the 10 Creator All-Rounder Pro Tools by action id.
    pub fn run_pro_tool(&mut self, action: &str) {
        log::info!("pro-tool: {action}");
        match action {
            // 1. Timeline Markers & YouTube Chapters
            "marker-toggle" => self.toggle_marker_at_playhead(),
            "marker-prev" => self.jump_marker(false),
            "marker-next" => self.jump_marker(true),
            "marker-copy-chapters" => self.copy_youtube_chapters(),
            "marker-clear" => self.clear_markers(),

            // 2. In / Out Loop Range & Loop Playback
            "loop-in" => self.set_loop_in(),
            "loop-out" => self.set_loop_out(),
            "loop-toggle" => self.toggle_loop_playback(),
            "loop-clear" => self.clear_loop_range(),

            // 3. Auto-Cut Silence (Smart Jump-Cut)
            "auto-cut-silence" => self.auto_cut_silence(),

            // 4. Copy & Paste Clip Attributes
            "copy-attributes" => self.copy_clip_attributes(),
            "paste-attributes" => self.paste_clip_attributes(),

            // 5. Ripple Close All Gaps
            "close-gaps" => self.close_all_gaps(),

            // 6. Track Select Forward
            "select-forward" => self.select_track_forward(),

            // 7. Quick Fades & Crossfade All Cuts
            "quick-fades" => self.apply_quick_fades(),
            "crossfade-all" => self.crossfade_all_cuts(),

            // 8. Clip Color Tags
            "tag-0" => self.set_selected_clips_tag(0),
            "tag-1" => self.set_selected_clips_tag(1),
            "tag-2" => self.set_selected_clips_tag(2),
            "tag-3" => self.set_selected_clips_tag(3),
            "tag-4" => self.set_selected_clips_tag(4),
            "tag-5" => self.set_selected_clips_tag(5),
            "tag-6" => self.set_selected_clips_tag(6),
            "tag-7" => self.set_selected_clips_tag(7),

            // 9. Preview Safe Zones, Rule-of-Thirds & 9:16 Crop Guides
            "guides-cycle" => {
                self.preview_guides = (self.preview_guides + 1) % 5;
            }
            "guides-0" => self.preview_guides = 0,
            "guides-1" => self.preview_guides = 1,
            "guides-2" => self.preview_guides = 2,
            "guides-3" => self.preview_guides = 3,
            "guides-4" => self.preview_guides = 4,

            // 10. Picture-in-Picture & Split-Screen Layouts
            "pip-reset" => self.apply_pip_layout(0),
            "pip-tl" => self.apply_pip_layout(1),
            "pip-tr" => self.apply_pip_layout(2),
            "pip-bl" => self.apply_pip_layout(3),
            "pip-br" => self.apply_pip_layout(4),
            "split-left" => self.apply_pip_layout(5),
            "split-right" => self.apply_pip_layout(6),
            "split-top" => self.apply_pip_layout(7),
            "split-bottom" => self.apply_pip_layout(8),
            _ => {}
        }
    }

    // ── Tool 1: Timeline Markers & YouTube Chapters ──

    pub fn toggle_marker_at_playhead(&mut self) {
        let timeline_id = self.timeline().id.clone();
        let at = self.playhead.max(0.0);
        let tol = (1.5 / self.frame_rate().max(1.0)).max(0.05);
        let existing_pos = self
            .tools_meta
            .markers
            .get(&timeline_id)
            .and_then(|list| list.iter().position(|m| (m.time - at).abs() <= tol));
        if let Some(pos) = existing_pos {
            if let Some(entries) = self.tools_meta.markers.get_mut(&timeline_id) {
                entries.remove(pos);
            }
            self.save_tools_meta();
            self.notify(&t("proTools.markerRemoved"), false);
            return;
        }
        let next_num = self
            .tools_meta
            .markers
            .get(&timeline_id)
            .map_or(0, Vec::len)
            + 1;
        let default_name = self
            .timeline()
            .clips
            .iter()
            .filter(|clip| {
                clip.start <= f64::from(at) && f64::from(at) < clip.start + clip.duration
            })
            .max_by_key(|clip| -self.row_of(&clip.track_id))
            .map(|clip| clip.name.trim().to_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| format!("{} {next_num}", t("proTools.chapter")));
        let color = ((next_num - 1) % 7 + 1) as i32;
        let entries = self.tools_meta.markers.entry(timeline_id).or_default();
        entries.push(TimelineMarker {
            time: at,
            name: default_name.clone(),
            color,
        });
        entries.sort_by(|a, b| a.time.total_cmp(&b.time));
        self.save_tools_meta();
        self.notify(&tf("proTools.markerAdded", &[&default_name]), false);
    }

    pub fn jump_marker(&mut self, forward: bool) {
        let markers = self.active_markers();
        if markers.is_empty() {
            self.notify(&t("proTools.noMarkers"), false);
            return;
        }
        let here = self.playhead;
        let eps = 0.02;
        let target = if forward {
            markers
                .iter()
                .find(|m| m.time > here + eps)
                .or_else(|| markers.first())
        } else {
            markers
                .iter()
                .rev()
                .find(|m| m.time < here - eps)
                .or_else(|| markers.last())
        };
        if let Some(marker) = target {
            let time = marker.time;
            self.seek(time);
        }
    }

    pub fn clear_markers(&mut self) {
        let timeline_id = self.timeline().id.clone();
        if let Some(list) = self.tools_meta.markers.get_mut(&timeline_id) {
            list.clear();
        }
        self.save_tools_meta();
        self.notify(&t("proTools.markersCleared"), false);
    }

    pub fn copy_youtube_chapters(&mut self) {
        let markers = self.active_markers();
        if markers.is_empty() {
            self.notify(&t("proTools.noMarkers"), true);
            return;
        }
        let mut lines = Vec::new();
        if markers.first().is_none_or(|m| m.time > 0.5) {
            lines.push(format!("00:00 {}", t("proTools.intro")));
        }
        for (idx, marker) in markers.iter().enumerate() {
            let total = marker.time.max(0.0).floor() as u64;
            let h = total / 3600;
            let m = (total / 60) % 60;
            let s = total % 60;
            let stamp = if h > 0 {
                format!("{h}:{m:02}:{s:02}")
            } else {
                format!("{m:02}:{s:02}")
            };
            let title = if marker.name.trim().is_empty() {
                format!("{} {}", t("proTools.chapter"), idx + 1)
            } else {
                marker.name.trim().to_owned()
            };
            lines.push(format!("{stamp} {title}"));
        }
        let text = lines.join("\n");
        self.chapters_clipboard = text.clone();
        self.chapters_clipboard_token = self.chapters_clipboard_token.wrapping_add(1);
        if let Some(session) = self.session.as_ref() {
            let file = std::path::Path::new(session.path()).join("youtube-chapters.txt");
            let _ = std::fs::write(file, &text);
        }
        self.notify(&tf("proTools.chaptersCopied", &[&lines.len()]), false);
    }

    // ── Tool 2: In / Out Range & Loop Playback ──

    pub fn set_loop_in(&mut self) {
        let timeline_id = self.timeline().id.clone();
        let at = self.playhead.max(0.0);
        let duration = self.duration();
        let entry = self.tools_meta.loop_ranges.entry(timeline_id).or_default();
        entry.in_point = Some(at);
        if entry.out_point.is_none_or(|out| out <= at + 0.05) {
            entry.out_point = Some((at + 5.0).min(duration.max(at + 1.0)));
        }
        self.loop_playback = true;
        self.save_tools_meta();
        self.notify(&t("proTools.loopInSet"), false);
    }

    pub fn set_loop_out(&mut self) {
        let timeline_id = self.timeline().id.clone();
        let at = self.playhead.max(0.1);
        let entry = self.tools_meta.loop_ranges.entry(timeline_id).or_default();
        if entry.in_point.is_none_or(|in_pt| in_pt >= at - 0.05) {
            entry.in_point = Some(0.0);
        }
        entry.out_point = Some(at);
        self.loop_playback = true;
        self.save_tools_meta();
        self.notify(&t("proTools.loopOutSet"), false);
    }

    pub fn toggle_loop_playback(&mut self) {
        if !self.loop_playback && self.active_loop_range().is_none() {
            let timeline_id = self.timeline().id.clone();
            let (in_pt, out_pt) = if !self.selection.is_empty() {
                let mut min_s = f64::INFINITY;
                let mut max_e = 0.0_f64;
                for id in &self.selection {
                    if let Some(clip) = self.clip(id) {
                        min_s = min_s.min(clip.start);
                        max_e = max_e.max(clip.start + clip.duration);
                    }
                }
                if max_e > min_s + 0.05 {
                    (min_s as f32, max_e as f32)
                } else {
                    (0.0, self.duration().max(1.0))
                }
            } else {
                (0.0, self.duration().max(1.0))
            };
            self.tools_meta.loop_ranges.insert(
                timeline_id,
                LoopRange {
                    in_point: Some(in_pt),
                    out_point: Some(out_pt),
                },
            );
            self.save_tools_meta();
        }
        self.loop_playback = !self.loop_playback;
    }

    pub fn clear_loop_range(&mut self) {
        let timeline_id = self.timeline().id.clone();
        self.tools_meta.loop_ranges.remove(&timeline_id);
        self.loop_playback = false;
        self.save_tools_meta();
        self.notify(&t("proTools.loopCleared"), false);
    }

    // ── Tool 3: Auto-Cut Silence (Smart Jump-Cut) ──

    pub fn auto_cut_silence(&mut self) {
        let targets = self.target_clips_or_under_playhead();
        let mut clips: Vec<Clip> = targets
            .iter()
            .filter_map(|id| self.clip(id).cloned())
            .filter(|clip| {
                !self.locked(&clip.track_id)
                    && self.clip_has_sound(clip)
                    && clip.duration >= 0.6
            })
            .collect();
        if clips.is_empty() {
            self.notify(&t("proTools.selectAudioOrVideoClip"), true);
            return;
        }
        // Sort clips from right to left so ripple-trimming a later clip never
        // shifts an earlier clip's timeline position before its cuts run.
        clips.sort_by(|a, b| b.start.total_cmp(&a.start));

        const STEP: f64 = 0.025;
        const THRESHOLD: f32 = 0.022;
        const MIN_SILENCE: f64 = 0.35;
        const PAD: f64 = 0.06;
        let min_keep = f64::from(MIN_DURATION) * 3.0;

        let mut commands = Vec::new();
        let mut cuts_made = 0usize;

        for clip in &clips {
            let key = art_key(&clip.media_id, clip.audio_stream);
            let pyramid = self.peaks.get(&key).cloned().or_else(|| {
                let media_item = self.project().media_by_id(&clip.media_id)?;
                let project_path = self.session.as_ref().map(|s| s.path());
                concat_host::media::peaks(&media_item.path, clip.audio_stream, project_path)
                    .ok()
                    .map(|p| Arc::new(Pyramid::of(p)))
            });
            let Some(peaks) = pyramid else {
                continue;
            };
            if peaks.finest().is_empty() {
                continue;
            }
            let speed = clip.speed.max(0.0625);
            let level = peaks.level_for((STEP * speed) as f32);
            let steps = ((clip.duration / STEP).floor() as usize).max(1);
            let mut silent_spans: Vec<(f64, f64)> = Vec::new();
            let mut run_start: Option<f64> = None;

            for i in 0..steps {
                let t0 = i as f64 * STEP;
                let t1 = ((i + 1) as f64 * STEP).min(clip.duration);
                let src0 = (clip.source_start + t0 * speed) as f32;
                let src1 = (clip.source_start + t1 * speed) as f32;
                let (low, high) = level.extremes(src0, src1);
                let amp = high.max(-low);
                if amp < THRESHOLD {
                    if run_start.is_none() {
                        run_start = Some(t0);
                    }
                } else if let Some(s0) = run_start.take() {
                    if t0 - s0 >= MIN_SILENCE {
                        let padded_start = if s0 <= STEP { 0.0 } else { s0 + PAD };
                        let padded_end = (t0 - PAD).max(padded_start);
                        if padded_end - padded_start >= MIN_SILENCE * 0.5 {
                            silent_spans.push((clip.start + padded_start, clip.start + padded_end));
                        }
                    }
                }
            }
            if let Some(s0) = run_start.take() {
                let t0 = clip.duration;
                if t0 - s0 >= MIN_SILENCE {
                    let padded_start = if s0 <= STEP { 0.0 } else { s0 + PAD };
                    let padded_end = t0;
                    if padded_end - padded_start >= MIN_SILENCE * 0.5 {
                        silent_spans.push((clip.start + padded_start, clip.start + padded_end));
                    }
                }
            }

            if silent_spans.is_empty() {
                continue;
            }

            // Process silent intervals from right to left on `clip.id`.
            // Because `SplitClips` keeps `clip.id` on the head (left) piece,
            // splitting at `s_end` and ripple-trimming the tail of `clip.id`
            // by `-(s_end - s_start)` removes each silent pause and ripples
            // all later pieces left without needing minted tail IDs.
            let mut current_end = clip.start + clip.duration;
            for &(s_start, s_end) in silent_spans.iter().rev() {
                let s_end = s_end.min(current_end);
                let s_start = s_start.max(clip.start);
                if s_end - s_start < min_keep {
                    continue;
                }
                let touches_head = s_start <= clip.start + min_keep;
                let touches_tail = s_end >= current_end - min_keep;

                if touches_head && touches_tail {
                    // Whole remaining piece is silent; leave at least a tiny sliver or skip
                    continue;
                } else if touches_tail {
                    let cut_len = current_end - s_start;
                    if current_end - clip.start - cut_len >= min_keep {
                        commands.push(Command::TrimClip {
                            clip_id: clip.id.clone(),
                            edge: TrimEdge::End,
                            delta: -cut_len,
                            ripple: true,
                        });
                        current_end = s_start;
                        cuts_made += 1;
                    }
                } else if touches_head {
                    let cut_len = s_end - clip.start;
                    if current_end - clip.start - cut_len >= min_keep {
                        commands.push(Command::TrimClip {
                            clip_id: clip.id.clone(),
                            edge: TrimEdge::Start,
                            delta: cut_len,
                            ripple: true,
                        });
                        cuts_made += 1;
                    }
                } else {
                    // Interior silent pause: split at s_end, then ripple-trim tail back to s_start.
                    commands.push(Command::SplitClips {
                        clip_ids: vec![clip.id.clone()],
                        time: s_end,
                    });
                    commands.push(Command::TrimClip {
                        clip_id: clip.id.clone(),
                        edge: TrimEdge::End,
                        delta: -(s_end - s_start),
                        ripple: true,
                    });
                    current_end = s_start;
                    cuts_made += 1;
                }
            }
        }

        if commands.is_empty() {
            self.notify(&t("proTools.noSilenceFound"), false);
            return;
        }
        self.flush_commit();
        self.apply(Command::Batch { commands });
        self.notify(&tf("proTools.silenceCutCount", &[&cuts_made]), false);
    }

    // ── Tool 4: Copy & Paste Clip Attributes ──

    pub fn copy_clip_attributes(&mut self) {
        let target = self
            .menu_target
            .clone()
            .or_else(|| self.sole_selection())
            .or_else(|| self.selection.first().cloned());
        let Some(clip) = target.and_then(|id| self.clip(&id).cloned()) else {
            self.notify(&t("proTools.selectClipFirst"), true);
            return;
        };
        let name = clip.name.clone();
        self.copied_attributes = Some(clip);
        self.notify(&tf("proTools.attributesCopied", &[&name]), false);
    }

    pub fn paste_clip_attributes(&mut self) {
        let Some(source) = self.copied_attributes.clone() else {
            self.notify(&t("proTools.noAttributesCopied"), true);
            return;
        };
        let targets = self.target_clips_or_under_playhead();
        if targets.is_empty() {
            self.notify(&t("proTools.selectClipFirst"), true);
            return;
        }
        let mut commands = Vec::new();
        let mut count = 0usize;
        for id in &targets {
            let Some(clip) = self.clip(id) else {
                continue;
            };
            if self.locked(&clip.track_id) {
                continue;
            }
            commands.push(Command::SetClipTransform {
                clip_id: id.clone(),
                scale: Some(source.scale),
                offset_x: Some(source.offset_x),
                offset_y: Some(source.offset_y),
                rotation: Some(source.rotation),
                stretch_x: Some(source.stretch_x),
                stretch_y: Some(source.stretch_y),
            });
            if (clip.speed - source.speed).abs() > 1e-4 {
                commands.push(Command::SetClipSpeed {
                    clip_id: id.clone(),
                    speed: source.speed,
                    ripple: false,
                });
            }
            commands.push(Command::UpdateClip {
                clip_id: id.clone(),
                patch: ClipPatch {
                    volume: Some(source.volume),
                    fade_in: Some(source.fade_in.min(clip.duration * 0.5)),
                    fade_out: Some(source.fade_out.min(clip.duration * 0.5)),
                    opacity: Some(source.opacity),
                    flip_h: Some(source.flip_h),
                    flip_v: Some(source.flip_v),
                    blend: Some(source.blend.clone()),
                    crop: Some(source.crop),
                    motion: Some(source.motion.clone()),
                    video_effects: Some(source.video_effects.clone()),
                    filters: Some(source.filters.clone()),
                    ..ClipPatch::default()
                },
            });
            count += 1;
        }
        if commands.is_empty() {
            return;
        }
        self.flush_commit();
        self.apply(Command::Batch { commands });
        self.notify(&tf("proTools.attributesPasted", &[&count]), false);
    }

    // ── Tool 5: Ripple Close All Gaps ──

    pub fn close_all_gaps(&mut self) {
        let timeline = self.timeline();
        let mut moves = Vec::new();
        for track in &timeline.tracks {
            if self.locked(&track.id) {
                continue;
            }
            let mut track_clips: Vec<&Clip> = timeline
                .clips
                .iter()
                .filter(|c| c.track_id == track.id)
                .map(AsRef::as_ref)
                .collect();
            track_clips.sort_by(|a, b| a.start.total_cmp(&b.start));
            let mut cursor = 0.0_f64;
            for clip in track_clips {
                if clip.start > cursor + 1e-3 {
                    moves.push(ClipMove {
                        clip_id: clip.id.clone(),
                        track_id: track.id.clone(),
                        start: cursor,
                    });
                }
                cursor += clip.duration;
            }
        }
        if moves.is_empty() {
            self.notify(&t("proTools.noGapsToClose"), false);
            return;
        }
        let count = moves.len();
        self.flush_commit();
        self.apply(Command::MoveClips { moves });
        self.notify(&tf("proTools.closedGapsCount", &[&count]), false);
    }

    // ── Tool 6: Track Select Forward (`A`) ──

    pub fn select_track_forward(&mut self) {
        self.flush_commit();
        let anchor = self
            .menu_target
            .as_deref()
            .and_then(|id| self.clip(id))
            .map(|clip| clip.start)
            .unwrap_or_else(|| f64::from(self.playhead))
            - 1e-3;
        let selected: Vec<String> = self
            .timeline()
            .clips
            .iter()
            .filter(|clip| !self.locked(&clip.track_id) && clip.start + clip.duration > anchor && clip.start >= anchor)
            .map(|clip| clip.id.clone())
            .collect();
        let count = selected.len();
        self.selection = selected;
        self.transition_selected = None;
        self.notify(&tf("proTools.selectedForwardCount", &[&count]), false);
    }

    // ── Tool 7: Quick Fades & Crossfade All Cuts ──

    pub fn apply_quick_fades(&mut self) {
        let targets = self.target_clips_or_under_playhead();
        let clips: Vec<Clip> = targets
            .iter()
            .filter_map(|id| self.clip(id).cloned())
            .filter(|clip| !self.locked(&clip.track_id))
            .collect();
        if clips.is_empty() {
            self.notify(&t("proTools.selectClipFirst"), true);
            return;
        }
        let all_have_fades = clips
            .iter()
            .all(|clip| clip.fade_in > 0.05 && clip.fade_out > 0.05);
        let commands: Vec<Command> = clips
            .iter()
            .map(|clip| {
                let dur = if all_have_fades {
                    0.0
                } else {
                    0.5_f64.min(clip.duration * 0.4)
                };
                Command::UpdateClip {
                    clip_id: clip.id.clone(),
                    patch: ClipPatch {
                        fade_in: Some(dur),
                        fade_out: Some(dur),
                        ..ClipPatch::default()
                    },
                }
            })
            .collect();
        let count = commands.len();
        self.flush_commit();
        self.apply(Command::Batch { commands });
        if all_have_fades {
            self.notify(&t("proTools.quickFadesCleared"), false);
        } else {
            self.notify(&tf("proTools.quickFadesApplied", &[&count]), false);
        }
    }

    pub fn crossfade_all_cuts(&mut self) {
        let transition_id = Catalogue::builtin()
            .of_kind(PackageKind::Transition)
            .find(|pkg| {
                let id = pkg.id();
                id.contains("dissolve") || id.contains("crossfade") || id.contains("fade")
            })
            .or_else(|| Catalogue::builtin().of_kind(PackageKind::Transition).next())
            .map(|pkg| pkg.id().to_owned())
            .unwrap_or_else(|| "concat.crossfade".to_owned());

        let selected_tracks: HashSet<String> = self
            .selection
            .iter()
            .filter_map(|id| self.clip(id).map(|c| c.track_id.clone()))
            .collect();

        let incoming_clips: Vec<Arc<Clip>> = self
            .timeline()
            .clips
            .iter()
            .filter(|clip| {
                !self.locked(&clip.track_id)
                    && clip.kind.is_visual()
                    && (selected_tracks.is_empty() || selected_tracks.contains(&clip.track_id))
                    && self.transition_duration(clip, 0.5).is_some()
            })
            .cloned()
            .collect();

        if incoming_clips.is_empty() {
            self.notify(&t("proTools.noAdjacentCuts"), true);
            return;
        }

        let all_have_transition = incoming_clips.iter().all(|c| c.transition_in.is_some());
        let commands: Vec<Command> = incoming_clips
            .iter()
            .filter_map(|clip| {
                let next_transition = if all_have_transition {
                    None
                } else {
                    let duration = self.transition_duration(clip, 0.5)?;
                    Some(Transition {
                        id: transition_id.clone(),
                        duration,
                        params: BTreeMap::new(),
                    })
                };
                Some(Command::UpdateClip {
                    clip_id: clip.id.clone(),
                    patch: ClipPatch {
                        transition_in: Some(next_transition),
                        ..ClipPatch::default()
                    },
                })
            })
            .collect();
        let count = commands.len();
        self.flush_commit();
        self.apply(Command::Batch { commands });
        self.notify(&tf("proTools.crossfadesApplied", &[&count]), false);
    }

    // ── Tool 8: Clip Color Tags ──

    pub fn set_selected_clips_tag(&mut self, color_index: i32) {
        let color_index = color_index.clamp(0, 7);
        let targets = self.target_clips_or_under_playhead();
        if targets.is_empty() {
            self.notify(&t("proTools.selectClipFirst"), true);
            return;
        }
        for id in &targets {
            if color_index == 0 {
                self.tools_meta.clip_tags.remove(id);
            } else {
                self.tools_meta.clip_tags.insert(id.clone(), color_index);
            }
        }
        self.save_tools_meta();
    }

    // ── Tool 10: Picture-in-Picture & Split-Screen Layouts ──

    pub fn apply_pip_layout(&mut self, preset: i32) {
        let targets = self.target_clips_or_under_playhead();
        let clips: Vec<Clip> = targets
            .iter()
            .filter_map(|id| self.clip(id).cloned())
            .filter(|clip| !self.locked(&clip.track_id) && clip.kind.is_visual())
            .collect();
        if clips.is_empty() {
            self.notify(&t("proTools.selectVisualClipFirst"), true);
            return;
        }

        let (scale, offset_x, offset_y, crop) = match preset {
            // 0: Reset Full Frame
            0 => (1.0, 0.0, 0.0, None),
            // 1: Top-Left PiP (32% scale, inset margin)
            1 => (0.32, -0.31, -0.31, None),
            // 2: Top-Right PiP
            2 => (0.32, 0.31, -0.31, None),
            // 3: Bottom-Left PiP
            3 => (0.32, -0.31, 0.31, None),
            // 4: Bottom-Right PiP
            4 => (0.32, 0.31, 0.31, None),
            // 5: Left 50/50 Split-Screen
            5 => (
                1.0,
                -0.25,
                0.0,
                Some(model::Crop {
                    left: 0.25,
                    right: 0.25,
                    top: 0.0,
                    bottom: 0.0,
                }),
            ),
            // 6: Right 50/50 Split-Screen
            6 => (
                1.0,
                0.25,
                0.0,
                Some(model::Crop {
                    left: 0.25,
                    right: 0.25,
                    top: 0.0,
                    bottom: 0.0,
                }),
            ),
            // 7: Top 50/50 Split-Screen
            7 => (
                1.0,
                0.0,
                -0.25,
                Some(model::Crop {
                    left: 0.0,
                    right: 0.0,
                    top: 0.25,
                    bottom: 0.25,
                }),
            ),
            // 8: Bottom 50/50 Split-Screen
            _ => (
                1.0,
                0.0,
                0.25,
                Some(model::Crop {
                    left: 0.0,
                    right: 0.0,
                    top: 0.25,
                    bottom: 0.25,
                }),
            ),
        };

        let mut commands = Vec::new();
        for clip in &clips {
            commands.push(Command::SetClipTransform {
                clip_id: clip.id.clone(),
                scale: Some(scale),
                offset_x: Some(offset_x),
                offset_y: Some(offset_y),
                rotation: Some(0.0),
                stretch_x: Some(1.0),
                stretch_y: Some(1.0),
            });
            commands.push(Command::UpdateClip {
                clip_id: clip.id.clone(),
                patch: ClipPatch {
                    crop: Some(crop),
                    ..ClipPatch::default()
                },
            });
        }
        self.flush_commit();
        self.apply(Command::Batch { commands });
        self.notify(&t("proTools.layoutApplied"), false);
    }

    // ── helpers ──

    fn target_clips_or_under_playhead(&self) -> Vec<String> {
        if !self.selection.is_empty() {
            return self.selection.clone();
        }
        if let Some(target) = &self.menu_target {
            if self.clip(target).is_some() {
                return vec![target.clone()];
            }
        }
        let at = f64::from(self.playhead);
        self.timeline()
            .clips
            .iter()
            .filter(|clip| {
                !self.locked(&clip.track_id)
                    && clip.start <= at
                    && at < clip.start + clip.duration
            })
            .max_by_key(|clip| -self.row_of(&clip.track_id))
            .map(|clip| vec![clip.id.clone()])
            .unwrap_or_default()
    }
}
