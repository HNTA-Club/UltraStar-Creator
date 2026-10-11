#ifndef QUDUETHIGHLIGHTER_H
#define QUDUETHIGHLIGHTER_H

#include <QSyntaxHighlighter>
#include <QTextCharFormat>
#include <QRegularExpression>
#include "song/QUDuetSupport.h"

class QUDuetHighlighter : public QSyntaxHighlighter {
    Q_OBJECT

public:
    explicit QUDuetHighlighter(QTextDocument *parent = nullptr);

    void setDuetEnabled(bool enabled);
    bool isDuetEnabled() const { return _duetEnabled; }

protected:
    void highlightBlock(const QString &text) override;

private:
    bool isDarkMode() const;

    bool _duetEnabled;
    QRegularExpression _tagRegex;
};

#endif // QUDUETHIGHLIGHTER_H
