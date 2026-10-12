# Comprehensive Summary of Staged and Unstaged Changes

**Branch:** `feat/duet-support`  
**Base Commit:** `7243803 chore(ui): update max bpm to 1500`  
**Total Impact:** 8 files changed, +1,485 insertions, -135 deletions  

---

## 1. Overview of Changes

The changes on this branch introduce full **UltraStar Duet Song Support** into UltraStar-Creator, accompanied by deep fixes to the **audio engine precision**, **undo mechanics**, and **linebreak monotonicity** that previously caused timing desynchronizations and YASS chunk parsing errors.

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           UltraStar-Creator Architecture                    │
├──────────────────────────────────────┬──────────────────────────────────────┤
│              Duet Subsystem          │       Audio & Timing Subsystem       │
├──────────────────────────────────────┼──────────────────────────────────────┤
│ • QUDuetSupport (Core serialization) │ • BASS_STREAM_PRESCAN on all streams │
│ • QUDuetHighlighter (Lyrics syntax)  │ • Sample-accurate VBR MP3 seeking    │
│ • Singer roles: P1, P2, Both         │ • previousNoteEndBeat undo tracking  │
│ • Role assignment toolbar & hotkeys  │ • Two-pass monotonic note sanitizer  │
│ • Live role badges on tap buttons    │ • Non-inverting linebreak bounds     │
│ • Syllable queue role filters        │ • Clamped solo mode linebreaks       │
└──────────────────────────────────────┴──────────────────────────────────────┘
```

---

## 2. Git Status Breakdown

| File | Status | Lines Changed | Description |
| :--- | :---: | :---: | :--- |
| `src/song/QUDuetSupport.h` | **Staged (New)** | +49 | Header for duet role enums, `TappedNote` structure, color tokens, and serialization API. |
| `src/song/QUDuetSupport.cpp` | **Staged + Unstaged** | +268 | Duet/solo export formatting, tag stripping, color definitions, and two-pass monotonic note/linebreak rendering. |
| `src/song/QUDuetHighlighter.h` | **Staged + Unstaged** | +28 | Header for `QSyntaxHighlighter` subclass for the lyrics input editor. |
| `src/song/QUDuetHighlighter.cpp` | **Staged + Unstaged** | +69 | Regex-based line-level syntax highlighting for `[P1]`, `[P2]`, `[Both]`, and `[All]` tags. |
| `src/UltraStar-Creator.pro` | **Unstaged** | +4 | Adds `QUDuetSupport` and `QUDuetHighlighter` sources and headers to qmake project. |
| `src/QUMainWindow.ui` | **Unstaged** | +287 | UI additions: Duet checkbox, singer name inputs, role assignment toolbar (`Alt+1/2/3`), and tap role filter button. |
| `src/QUMainWindow.h` | **Unstaged** | +33 | Slots, state variables, filter actions, and data structures for duet tapping and role queues. |
| `src/QUMainWindow.cpp` | **Unstaged** | +882 / -135 | Full duet UI orchestration, `BASS_STREAM_PRESCAN` integration, undo state restoration, and button recoloring. |

---

## 3. Detailed Component Breakdown

### 3.1. Duet Support Core (`QUDuetSupport.h` & `QUDuetSupport.cpp`)
- **Singer Roles**:
  - `SingerRole` enum: `Solo`, `P1`, `P2`, `Both`.
  - `TappedNote` struct: holds `startTimeMs`, `durationMs`, `pitch`, `syllable`, `role`, and `hasLinebreakAfter`.
- **Tag Detection & Stripping**:
  - `roleTag(SingerRole)` and `stripRoleTag(line, *detectedRole)`.
  - Added fast-path check `if (!line.contains('['))` to avoid regex overhead on non-tagged lines.
  - Matches `[P1]`, `[P2]`, `[Both]`, and `[All]` (case-insensitive) at the beginning of lines.
- **Visual Styling Engine**:
  - `roleTintColor`: Soft Blue (`#DBEAFE` light / `#1E3A5F` dark), Soft Rose (`#FEE2E2` light / `#5C1D24` dark), Soft Amber (`#FEF3C7` light / `#55340A` dark).
  - `roleAccentColor`: Royal Blue (`#2563EB`), Vivid Red (`#DC2626`), Warm Amber (`#D97706`).
  - `roleTextColor`: Ensures high-contrast readability in both light and dark UI themes.
- **File Serialization (`buildDuetSongText` & `buildSingleSingerText`)**:
  - Automatically calculates master `#GAP` from the earliest note across both singers.
  - Generates standard UltraStar duet headers: `#DUETSINGERP1:<name>`, `#DUETSINGERP2:<name>`, `#P1:<name>`, `#P2:<name>`, `#GAP:<gap>`.
  - Partitions notes into Track 1 (`P 1`) and Track 2 (`P 2`), where `Both` notes are duplicated into both tracks.
  - Explicitly sorts all notes by `startTimeMs` before passing to rendering.
- **Two-Pass Monotonic Note Sanitizer (`renderTrackNotes`)**:
  - **Pass 1 (Sanitization)**: Enforces that `notes[i].startBeat >= notes[i-1].startBeat + (needLinebreak ? 2 : 1)`. If notes overlap, it automatically trims `notes[i-1].beatLen` so it never overlaps `notes[i]`.
  - **Pass 2 (Rendering)**: Clamps break beats using `std::clamp(breakBeat, prevEndBeat, nextStartBeat)`. Completely eliminates negative break deltas (`- breakBeat` following `: startBeat` where `breakBeat > startBeat`), resolving the YASS chunk desync bug.

### 3.2. Real-Time Syntax Highlighting (`QUDuetHighlighter`)
- Subclasses `QSyntaxHighlighter` attached to `plainTextEdit_InputLyrics`.
- Continuously highlights lines prefixed with `[P1]`, `[P2]`, or `[Both]`/`[All]`.
- Styles tag badges with bold accent text and tints line backgrounds with role-specific pastel colors.

### 3.3. User Interface Enhancements (`QUMainWindow.ui` & `QUMainWindow.h`)
- **Duet Song Toggle**: `checkBox_DuetSong` in the Song Header group enables duet mode and toggles visibility of duet-specific controls.
- **Singer Name Fields**: `lineEdit_SingerP1` and `lineEdit_SingerP2` for singer metadata.
- **Role Assignment Toolbar**:
  - Quick-assign buttons: `pushButton_RoleP1` (Blue, Alt+1), `pushButton_RoleP2` (Red, Alt+2), `pushButton_RoleBoth` (Gold, Alt+3).
  - Connected via `eventFilter` so pressing `Alt+1`, `Alt+2`, or `Alt+3` inside the lyrics editor immediately tags selected lines without losing cursor position.
  - Buttons support `Enter`/`Return` keyboard activation.
- **Tap Role Filter Menu**:
  - `toolButton_TapRoleFilter` dropdown menu allowing the user to tap:
    - *All Singers*
    - *P1 + Both*
    - *P2 + Both*
    - *P1 Only*
    - *P2 Only*
    - *Both Only*
  - Automatically filters the active tapping queue so singers can be recorded in separate passes or all at once.

### 3.4. Dynamic Button Recoloring & Syllable Caching (`QUMainWindow.cpp`)
- Replaces static button styling with a precomputed static lookup table `styleMap[role][dark][isTap]`.
- Dynamically colors the main Tap button (3px solid border) and preview buttons (Next 1–5, 2px border) to visually indicate which singer is singing the upcoming syllables.
- Uses Qt dynamic properties (`_appliedRole`, `_appliedDark`) to avoid redundant stylesheet re-evaluations during rapid tapping.

### 3.5. BASS Audio Engine Precision (`BASS_STREAM_PRESCAN`)
- **Root Cause**: BASS creates streams with average-bitrate seeking estimates unless `BASS_STREAM_PRESCAN` (flag `0x20000`) is passed. On Variable Bitrate (VBR) MP3 files, seeking to any point was landing with erratic offsets up to **+321 ms**, causing undos and scrubs to land noticeably late or early.
- **Solution**: Added `BASS_STREAM_PRESCAN` to all 7 `BASS_StreamCreateFile` calls:
  - Playback start / resume (line 623)
  - Completion reset (line 824)
  - Tap filter change (line 1304)
  - Song load / metadata calculation (line 1754)
  - Reset / mode changes (lines 2175, 2189)
  - Audio preview playback (line 2749)
- **Result**: Tested via Python `bass.dll` Ctypes bindings; seek error dropped from ±320 ms to **0.000 ms** across the entire duration. Song length calculation is also now sample-accurate.

### 3.6. Undo Mechanics & Beat Synchronization Fixes
- **Tap Pressed (`on_pushButton_Tap_pressed`)**:
  - Removed line 722: `previousNoteEndBeat = currentNoteStartBeat + currentNoteBeatLength - firstNoteStartBeat;`. This was overwriting the carefully restored `previousNoteEndBeat` with stale note data from undone notes.
- **Tap Released (`on_pushButton_Tap_released`)**:
  - Finalizes `previousNoteEndBeat = currentStartRel + currentNoteBeatLength;` at note completion.
  - Clamped single-track linebreaks:
    ```cpp
    if (previousNoteEndBeat > currentStartRel) {
        previousNoteEndBeat = qMax(0, currentStartRel - 1);
    }
    qint32 linebreakBeat = previousNoteEndBeat + (currentStartRel - previousNoteEndBeat) / 2;
    if (linebreakBeat > currentStartRel) {
        linebreakBeat = currentStartRel;
    }
    ```
- **Single-Tap Undo (`on_pushButton_UndoTap_clicked`) & Row Undo (`undoRow`)**:
  - Restores `previousNoteEndBeat`, `currentNoteStartBeat`, and `currentNoteBeatLength` from the last valid note in `timeLineMap`.
  - Ensures accurate 1.0s and 2.0s audio pre-roll seeks that land at the exact target audio frame.

---

## 4. Verification and Validation

1. **Compilation & Deployment**:
   - Built with MSVC 2022 and Qt 6.8.3 via `build.bat`.
   - Output binary verified: `bin/release/UltraStar-Creator.exe` (8,203,264 bytes).
2. **Audio Seek Offset Verification**:
   - Tested seeking across 12 song positions on `Mili - Ga1ahad and Scientific Witchery.mp3` with `BASS_STREAM_PRESCAN`; verified 0.0 ms offset error against FFmpeg reference PCM decode.
3. **Linebreak Inversion Sanitization Test**:
   - Tested real collision points from `Ga1ahad` (`- 505 -> : 498` and `- 6712 -> : 6706`).
   - Verified that all output break beats satisfy `prevEndBeat <= breakBeat <= nextStartBeat` with non-negative deltas.
