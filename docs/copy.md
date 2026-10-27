# Robot lines and UI labels (v0)

Draft by Copilot (job #230), edited. Short (≤ 12 words), warm, read aloud by TTS. `{name}` = `ROBOT_NAME` = Mate.
Mate is male: Russian lines use the masculine forms (нашёл, показал).

| Situation | English | Русский |
|---|---|---|
| First launch | Hello! I'm {name}. To talk, press and hold Right Ctrl, or press and hold me. | Здравствуйте! Я {name}. Зажмите правый Ctrl или меня, чтобы поговорить. |
| Listening | I'm listening. | Слушаю. |
| Looking | One moment. I'm looking only at this window. | Секунду. Смотрю только на это окно. |
| Found it | Here it is. Look where I'm pointing. | Вот она. Посмотрите, куда я показываю. |
| Step | Step {n} of {m}: {instruction} | Шаг {n} из {m}: {instruction} |
| Right click | Yes, that's it. Well done. | Да, всё верно. Отлично. |
| Wrong place | Almost. Click right where I'm pointing. | Почти. Нажмите туда, куда я показываю. |
| Can't find | I can't see that here. Let's open the right window first. | Здесь этого не вижу. Давайте сначала откроем нужное окно. |
| Private window | This window may show bank details or a password. May I look? | Тут могут быть данные банка или пароль. Можно посмотреть? |
| Password covered | I covered your password before looking. | Я закрыл ваш пароль, прежде чем смотреть. |
| Login page | A password may be visible here. Hide it, then I'll look. | Тут может быть виден пароль. Скройте его, и я посмотрю. |
| Do it for me? | Should I press this button for you? | Нажать эту кнопку за вас? |
| Won't press | I don't press Pay or Delete. I'll show you where it is. | «Оплатить» и «Удалить» я не нажимаю. Но покажу, где это. |
| Network error | I can't connect right now. Let's try again in a minute. | Сейчас нет связи. Попробуем ещё раз через минутку. |
| Guide done | All done. Nice work! | Всё готово. Вы молодец! |
| Too many steps | This is taking too long. Let's try another way. | Что-то долго получается. Давайте попробуем по-другому. |
| Calm mode | Calm mode is on. I'll keep sounds gentle. | Спокойный режим. Звуки будут тихими. |
| Fun mode | Fun mode is on. Let's have some fun! | Весёлый режим. Будет веселее! |
| Sleep | I'll wait in the corner. Call me anytime. | Подожду в уголке. Зовите в любой момент. |
| Wake | I'm back. How can I help? | Я снова здесь. Чем помочь? |
| Protected window | This window is protected. I can't see inside it. | Это окно защищено, я не вижу, что в нём. |
| Security prompt | Please answer the Windows security window first. | Сначала ответьте в окне безопасности Windows. |

## Do it for me

Mate presses at most one button per spoken yes, never Pay, Delete, money transfers or passwords.

| Locale key | English | Русский |
|---|---|---|
| `robot.do_it_for_me` | Should I press this button for you? Say yes or no. | Нажать эту кнопку за вас? Скажите «да» или «нет». |
| `robot.pressing` | All right, pressing it now. | Хорошо, сейчас нажму. |
| `robot.press_cancelled` | Of course. I won't press it. Take your time. | Конечно, не буду нажимать. Не спешите. |
| `robot.press_failed` | I couldn't press it safely. Please press it yourself. | Не получилось нажать безопасно. Нажмите, пожалуйста, сами. |
| `robot.wont_press` | I don't press Pay or Delete. I'll show you where it is. | «Оплатить» и «Удалить» я не нажимаю. Но покажу, где это. |

## Landing page: for the adult child who installs Mate

Drafts by DeepSeek (job #404), edited; picked over oss (#403). Pick one on Day 6 (`edge.md` §4, move 16).

| English | Русский |
|---|---|
| Mate lights the button. Your parent clicks it. | Mate подсвечивает кнопку. Нажимает ваш родитель. |
| He never presses Pay or Delete. He only shows the way. | Он не нажимает «Оплатить» и «Удалить». Только показывает. |
| Peace of mind for you, confidence for your parent. | Спокойствие вам, уверенность вашим родителям. |

## Tray menu

| English | Русский |
|---|---|
| Show robot | Показать робота |
| Hide robot | Спрятать робота |
| Calm sounds / Fun sounds | Спокойные звуки / Весёлые звуки |
| Settings | Настройки |
| About | О программе |
| Quit | Выйти |

## Settings

| English | Русский |
|---|---|
| Robot size | Размер робота |
| Voice speed | Скорость речи |
| Sound volume | Громкость звуков |
| Sound mode | Режим звуков |
| Push-to-talk key | Клавиша для разговора |
| Hold to talk / Press to start and stop | Удерживать / Нажать, чтобы начать и закончить |
| Language: automatic | Язык: автоматически |
| Reduce motion | Меньше движения |
| Start with Windows | Запускать вместе с Windows |
| Invite code / Your Meta API key | Код приглашения / Ваш ключ Meta API |
| How {name} thinks | Как думает {name} |
| Quick (Muse Spark 1.3) | Быстро (Muse Spark 1.3) |
| Answers fast. Best for everyday help. | Отвечает быстро. Для повседневных дел. |
| Thorough (Muse Wonder 1.0) | Вдумчиво (Muse Wonder 1.0) |
| Thinks longer about tricky tasks. Slower. | Дольше думает над сложными задачами. Медленнее. |

Model choice (D40): Spark is the default; Wonder only by the user's choice, never by voice.

## Pitch lines (candidates, 2026-10-27)

- Hold Right Ctrl, ask aloud; Mate points to the right button.
- Mate flies to the right button, highlights it, and speaks your language.
- A golden ring marks the exact button in any Windows app.
- Удерживайте правый Ctrl, спросите вслух — Mate покажет нужную кнопку.
- Mate подлетает к нужной кнопке, подсвечивает её и говорит на вашем языке.

Proposed by DeepSeek, picked by the lead; final choice after the rules are read (D42).
