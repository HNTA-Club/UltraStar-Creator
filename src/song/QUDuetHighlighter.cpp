#include "QUDuetHighlighter.h"
#include <QApplication>
#include <QPalette>
#include <QFont>

QUDuetHighlighter::QUDuetHighlighter(QTextDocument *parent)
    : QSyntaxHighlighter(parent)
    , _duetEnabled(false)
    , _tagRegex(R"(^\s*\[(P1|P2|Both|All)\])", QRegularExpression::CaseInsensitiveOption)
{
}

void QUDuetHighlighter::setDuetEnabled(bool enabled) {
    if (_duetEnabled != enabled) {
        _duetEnabled = enabled;
        rehighlight();
    }
}

bool QUDuetHighlighter::isDarkMode() const {
    const QPalette &pal = QApplication::palette();
    // If base or window background is dark, we are in dark mode
    return pal.color(QPalette::Base).lightness() < 128 || pal.color(QPalette::Window).lightness() < 128;
}

void QUDuetHighlighter::highlightBlock(const QString &text) {
    if (!_duetEnabled) {
        return;
    }
    int len = text.length();
    int i = 0;
    while (i < len && text.at(i).isSpace()) {
        ++i;
    }
    if (i >= len || text.at(i) != '[') {
        return;
    }

    auto match = _tagRegex.match(text);
    if (!match.hasMatch()) {
        return;
    }

    SingerRole role = QUDuetSupport::stringToRole(match.captured(1));
    if (role == SingerRole::Solo) {
        return;
    }

    int tagStart = match.capturedStart();
    int tagLen = match.capturedLength();

    bool darkMode = isDarkMode();
    QColor tint = QUDuetSupport::roleTintColor(role, darkMode);
    QColor textColor = QUDuetSupport::roleTextColor(role, darkMode);
    QColor accent = QUDuetSupport::roleAccentColor(role);

    // Entire line format
    QTextCharFormat lineFormat;
    lineFormat.setBackground(tint);
    lineFormat.setForeground(textColor);
    setFormat(0, text.length(), lineFormat);

    // Highlight tag specifically (bold, accent color)
    QTextCharFormat tagFormat;
    tagFormat.setBackground(tint);
    tagFormat.setForeground(accent);
    tagFormat.setFontWeight(QFont::Bold);
    setFormat(tagStart, tagLen, tagFormat);
}
