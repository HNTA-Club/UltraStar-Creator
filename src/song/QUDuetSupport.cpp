#include "QUDuetSupport.h"
#include <QRegularExpression>
#include <algorithm>

QString QUDuetSupport::roleToString(SingerRole role) {
    switch (role) {
        case SingerRole::P1: return QStringLiteral("P1");
        case SingerRole::P2: return QStringLiteral("P2");
        case SingerRole::Both: return QStringLiteral("Both");
        case SingerRole::Solo:
        default: return QStringLiteral("Solo");
    }
}

SingerRole QUDuetSupport::stringToRole(const QString &str) {
    QString s = str.trimmed().toUpper();
    if (s == "P1" || s == "[P1]" || s == "1") return SingerRole::P1;
    if (s == "P2" || s == "[P2]" || s == "2") return SingerRole::P2;
    if (s == "BOTH" || s == "[BOTH]" || s == "3" || s == "ALL") return SingerRole::Both;
    return SingerRole::Solo;
}

QString QUDuetSupport::roleTag(SingerRole role) {
    switch (role) {
        case SingerRole::P1: return QStringLiteral("[P1]");
        case SingerRole::P2: return QStringLiteral("[P2]");
        case SingerRole::Both: return QStringLiteral("[Both]");
        case SingerRole::Solo:
        default: return QString();
    }
}

QString QUDuetSupport::stripRoleTag(const QString &line, SingerRole *detectedRole) {
    if (!line.contains('[')) {
        if (detectedRole) {
            *detectedRole = SingerRole::Solo;
        }
        return line;
    }

    static const QRegularExpression tagRegex(R"(^\s*\[(P1|P2|Both|All)\]\s*)", QRegularExpression::CaseInsensitiveOption);
    auto match = tagRegex.match(line);
    if (match.hasMatch()) {
        if (detectedRole) {
            *detectedRole = stringToRole(match.captured(1));
        }
        return line.mid(match.capturedLength());
    }

    if (detectedRole) {
        *detectedRole = SingerRole::Solo;
    }
    return line;
}

QColor QUDuetSupport::roleTintColor(SingerRole role, bool isDarkMode) {
    switch (role) {
        case SingerRole::P1:
            // Light: Soft Blue (#DBEAFE), Dark: Deep Navy (#1E3A5F)
            return isDarkMode ? QColor(30, 58, 95) : QColor(219, 234, 254);
        case SingerRole::P2:
            // Light: Soft Rose (#FEE2E2), Dark: Deep Wine (#5C1D24)
            return isDarkMode ? QColor(92, 29, 36) : QColor(254, 226, 226);
        case SingerRole::Both:
            // Light: Soft Amber (#FEF3C7), Dark: Deep Bronze/Amber (#55340A)
            return isDarkMode ? QColor(85, 52, 10) : QColor(254, 243, 199);
        case SingerRole::Solo:
        default:
            return isDarkMode ? QColor(30, 30, 30) : QColor(255, 255, 255);
    }
}

QColor QUDuetSupport::roleAccentColor(SingerRole role) {
    switch (role) {
        case SingerRole::P1:
            return QColor(37, 99, 235); // Royal Blue #2563EB
        case SingerRole::P2:
            return QColor(220, 38, 38); // Vivid Red #DC2626
        case SingerRole::Both:
            return QColor(217, 119, 6); // Warm Amber / Gold #D97706
        case SingerRole::Solo:
        default:
            return QColor(100, 116, 139);
    }
}

QColor QUDuetSupport::roleTextColor(SingerRole role, bool isDarkMode) {
    Q_UNUSED(role);
    // In light mode with pale tint, text must be dark (#1E293B)
    // In dark mode with deep tint, text must be light (#F8FAFC)
    return isDarkMode ? QColor(248, 250, 252) : QColor(30, 41, 59);
}

static QString renderTrackNotes(const QList<TappedNote> &trackNotes, double gapMs, double bpm) {
    if (trackNotes.isEmpty()) {
        return QString();
    }

    struct RenderNote {
        qint64 startBeat;
        qint64 beatLen;
        int pitch;
        QString syllable;
        bool needLinebreakAfter;
    };

    QList<RenderNote> notes;
    notes.reserve(trackNotes.size());

    for (const auto &n : trackNotes) {
        RenderNote rn;
        rn.startBeat = qMax((qint64)0, (qint64)std::round((n.startTimeMs - gapMs) * bpm / 15000.0));
        rn.beatLen = qMax((qint64)1, (qint64)std::round(n.durationMs * bpm / 15000.0));
        rn.pitch = n.pitch;
        rn.syllable = n.syllable;
        rn.needLinebreakAfter = (rn.syllable.endsWith('\n') || n.hasLinebreakAfter);
        rn.syllable.remove('\n').remove("•").remove("+");
        notes.append(rn);
    }

    // Pass 1: Ensure monotonic start beats and prevent note overlaps
    for (int i = 1; i < notes.size(); ++i) {
        qint64 minStart = notes[i - 1].startBeat + (notes[i - 1].needLinebreakAfter ? 2 : 1);
        if (notes[i].startBeat < minStart) {
            notes[i].startBeat = minStart;
        }
        qint64 maxPrevEnd = notes[i].startBeat - (notes[i - 1].needLinebreakAfter ? 1 : 0);
        if (notes[i - 1].startBeat + notes[i - 1].beatLen > maxPrevEnd) {
            notes[i - 1].beatLen = qMax((qint64)1, maxPrevEnd - notes[i - 1].startBeat);
        }
    }

    // Pass 2: Render output lines with guaranteed valid break beats
    QStringList out;
    for (int i = 0; i < notes.size(); ++i) {
        const auto &rn = notes[i];
        out << QString(": %1 %2 %3 %4").arg(rn.startBeat).arg(rn.beatLen).arg(rn.pitch).arg(rn.syllable);
        if (rn.needLinebreakAfter && i + 1 < notes.size()) {
            qint64 prevEnd = rn.startBeat + rn.beatLen;
            qint64 nextStart = notes[i + 1].startBeat;
            qint64 breakBeat = (nextStart > prevEnd) ? (prevEnd + (nextStart - prevEnd) / 2) : prevEnd;
            breakBeat = std::clamp(breakBeat, prevEnd, nextStart);
            out << QString("- %1").arg(breakBeat);
        }
    }

    return out.join("\n");
}

QString QUDuetSupport::buildDuetSongText(const QMap<double, QString> &headerMap,
                                         const QList<TappedNote> &notes,
                                         double bpm,
                                         const QString &singerP1,
                                         const QString &singerP2) {
    QStringList lines;

    double minStartMs = 0.0;
    bool hasNotes = !notes.isEmpty();
    if (hasNotes) {
        minStartMs = notes.first().startTimeMs;
        for (const auto &n : notes) {
            if (n.startTimeMs < minStartMs) {
                minStartMs = n.startTimeMs;
            }
        }
    }
    double gap = std::round(minStartMs / 10.0) * 10.0;

    for (auto it = headerMap.begin(); it != headerMap.end(); ++it) {
        if (it.key() < 0) {
            QString val = it.value();
            if (val.startsWith("#GAP:", Qt::CaseInsensitive)) {
                continue;
            }
            if (val.startsWith("#DUETSINGERP", Qt::CaseInsensitive) || val.startsWith("#P1:", Qt::CaseInsensitive) || val.startsWith("#P2:", Qt::CaseInsensitive)) {
                continue;
            }
            lines << val;
        }
    }

    lines << QString("#GAP:%1").arg(QString::number(gap, 'f', 0));
    lines << QString("#DUETSINGERP1:%1").arg(singerP1);
    lines << QString("#P1:%1").arg(singerP1);
    lines << QString("#DUETSINGERP2:%1").arg(singerP2);
    lines << QString("#P2:%1").arg(singerP2);

    if (!hasNotes) {
        lines << "E";
        return lines.join("\n") + "\n";
    }

    QList<TappedNote> p1Notes;
    p1Notes.reserve(notes.size());
    for (const auto &n : notes) {
        if (n.role == SingerRole::P1 || n.role == SingerRole::Both || n.role == SingerRole::Solo) {
            p1Notes << n;
        }
    }
    std::sort(p1Notes.begin(), p1Notes.end(), [](const TappedNote &a, const TappedNote &b) {
        return a.startTimeMs < b.startTimeMs;
    });
    lines << "P 1";
    QString t1 = renderTrackNotes(p1Notes, gap, bpm);
    if (!t1.isEmpty()) lines << t1;

    QList<TappedNote> p2Notes;
    p2Notes.reserve(notes.size());
    for (const auto &n : notes) {
        if (n.role == SingerRole::P2 || n.role == SingerRole::Both) {
            p2Notes << n;
        }
    }
    std::sort(p2Notes.begin(), p2Notes.end(), [](const TappedNote &a, const TappedNote &b) {
        return a.startTimeMs < b.startTimeMs;
    });
    lines << "P 2";
    QString t2 = renderTrackNotes(p2Notes, gap, bpm);
    if (!t2.isEmpty()) lines << t2;

    lines << "E";
    return lines.join("\n") + "\n";
}

QString QUDuetSupport::buildSingleSingerText(const QMap<double, QString> &headerMap,
                                             const QList<TappedNote> &notes,
                                             double bpm,
                                             SingerRole targetSinger) {
    QStringList lines;
    double minStartMs = 0.0;
    bool hasNotes = !notes.isEmpty();
    if (hasNotes) {
        minStartMs = notes.first().startTimeMs;
        for (const auto &n : notes) {
            if (n.startTimeMs < minStartMs) {
                minStartMs = n.startTimeMs;
            }
        }
    }
    double gap = std::round(minStartMs / 10.0) * 10.0;

    for (auto it = headerMap.begin(); it != headerMap.end(); ++it) {
        if (it.key() < 0) {
            QString val = it.value();
            if (val.startsWith("#GAP:", Qt::CaseInsensitive) || val.startsWith("#DUETSINGERP", Qt::CaseInsensitive) || val.startsWith("#P1:", Qt::CaseInsensitive) || val.startsWith("#P2:", Qt::CaseInsensitive)) {
                continue;
            }
            lines << val;
        }
    }
    lines << QString("#GAP:%1").arg(QString::number(gap, 'f', 0));

    QList<TappedNote> filtered;
    filtered.reserve(notes.size());
    for (const auto &n : notes) {
        if (n.role == targetSinger || n.role == SingerRole::Both || n.role == SingerRole::Solo) {
            filtered << n;
        }
    }
    std::sort(filtered.begin(), filtered.end(), [](const TappedNote &a, const TappedNote &b) {
        return a.startTimeMs < b.startTimeMs;
    });

    QString rendered = renderTrackNotes(filtered, gap, bpm);
    if (!rendered.isEmpty()) lines << rendered;
    lines << "E";
    return lines.join("\n") + "\n";
}
