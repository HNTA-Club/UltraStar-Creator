#ifndef QUDUETSUPPORT_H
#define QUDUETSUPPORT_H

#include <QString>
#include <QStringList>
#include <QList>
#include <QColor>

enum class SingerRole {
    Solo = 0,
    P1 = 1,
    P2 = 2,
    Both = 3
};

struct TappedNote {
    double startTimeMs = 0.0;
    double durationMs = 0.0;
    int pitch = 0;
    QString syllable;
    SingerRole role = SingerRole::Solo;
    bool hasLinebreakAfter = false;
};

class QUDuetSupport {
public:
    static QString roleToString(SingerRole role);
    static SingerRole stringToRole(const QString &str);
    static QString roleTag(SingerRole role);
    static QString stripRoleTag(const QString &line, SingerRole *detectedRole = nullptr);

    // Color definitions for UI (high contrast in light and dark modes)
    static QColor roleTintColor(SingerRole role, bool isDarkMode);
    static QColor roleAccentColor(SingerRole role);
    static QColor roleTextColor(SingerRole role, bool isDarkMode);

    static QString buildDuetSongText(const QMap<double, QString> &headerMap,
                                     const QList<TappedNote> &notes,
                                     double bpm,
                                     const QString &singerP1,
                                     const QString &singerP2);

    static QString buildSingleSingerText(const QMap<double, QString> &headerMap,
                                         const QList<TappedNote> &notes,
                                         double bpm,
                                         SingerRole targetSinger);
};

#endif // QUDUETSUPPORT_H
