# Ograniczenia i zasady — wersja 0.5 (16 października 2026)

Wersja 0.5 dodaje odczyt panelu stanowiska broni MTC i pełnej mapy (klawisz M), korektę celowania na żywo, skróty punktów mapy, moduł ognia bezpośredniego, rakiety ze śmigłowca, salwę MLRS, dynamiczne przechwycenie beta i nagrywanie testowe za zgodą. Czytniki sprawdzono na zrzutach prawdziwej gry, ale **zgodność obliczeń z trafieniami w aktualnym MTC nie została potwierdzona**.

## Materiały i pochodzenie danych

Pierwotny pakiet zawiera dwie fotografie monitora (BM-21: czas 27:42, kompas 138; śmigłowiec: 29:33). Nie są to klatki klienta Roblox i nie potwierdzają współrzędnych ani parametrów broni.

`tests/fixtures/live-2026-10-15` zawiera wycinki zrzutów gry wykonanych za zgodą użytkownika (GDI, 1920×1080, DPI 125%, place 95721658376580, tryb Frontlines, mapa Zone 11). Oczekiwane wartości i współrzędne wycinków opisuje `manifest.json`. Pełnych klatek z gry nie ma w repozytorium ani w pakiecie wydania.

Dane społeczności pochodzą z [grand-hawk/artillery-calculator, commit 39a7af7](https://github.com/grand-hawk/artillery-calculator/tree/39a7af7689524e57ca2326f3b998cf123df95426) na [licencji MIT](https://github.com/grand-hawk/artillery-calculator/blob/39a7af7689524e57ca2326f3b998cf123df95426/LICENSE.md). 15 października 2026 HEAD repozytorium wskazywał nadal ten commit, więc dane nie wymagały aktualizacji. Commit nie oznacza wersji gry. [Rejestr źródeł](community-sources.md).

Źródło odwołuje się do doświadczenia `9520328730`. Jest to starsze MTC4, przeniesione; aktualne MTC to [place 95721658376580](https://www.roblox.com/games/95721658376580). Licencja rastrów map i heightmap z `maps.artillery-calculator.com` jest nieznana. Aplikacja pobiera je wyłącznie na żądanie do lokalnego cache, a ZIP ich nie zawiera. [Zasoby map](community-map-assets.md).

## Profile i obliczenia

Profile społeczności mają `status: experimental`, `calibrated: false` i `sourceAcknowledged: false`. Potwierdzenie źródła dla pocisku pozwala liczyć, ale nie oznacza verified. Niezgodne jednostki blokują wynik. Profil kierowany pozostaje zablokowany.

Modele oporu i napędu wymagają jawnych współczynników z lokalnej kalibracji (beta). Dynamiczne przechwycenie z oporem/napędem jest numeryczne (RK4 + Newton) i zakłada stały ruch celu. Salwa MLRS wymaga podanych parametrów rozrzutu; brak danych blokuje elipsy. Moduł ognia bezpośredniego zakłada grawitację kalkulatora społeczności i jednostkę „m/s” z ekranu Loadout jako metry gry/s — to niezweryfikowane założenia. Śmigłowiec: start rakiety wzdłuż wektora lotu i dziedziczenie prędkości są założeniami. [Model](projectile-dynamics.md).

## Granice analizy obrazu

WGC i GDI korzystają z publicznych API Windows i wybranego okna. HDR, ekskluzywny fullscreen, ochrona obrazu lub brak sterownika mogą dać błąd albo niewiarygodną klatkę.

Czytnik panelu broni rozpoznaje nazwy i kąty ELEVATION/AZIMUTH wzorcami LED wyuczonymi na innych klatkach niż testowe. RANGE „0000” oznacza brak pomiaru. Konwencja AZIMUTH względem azymutu mapy wymaga sprawdzenia w grze. Czytnik mapy M identyfikuje mapę po napisie „Grid size” i rozmiarze katalogowym; kilka map o tym samym polu daje stan niejednoznaczny. OCR ekranu Loadout nie podaje nazwy broni. Procenty pewności są heurystyką.

Matcher widoku mapy osiąga na syntetycznych klatkach 98–100% dla pełnej mapy, mniej dla zoomu i minimapy ([benchmark](map-matching-benchmark.md)). Wynik nie dowodzi aktualności tekstur. Detektor markerów wyszukuje widoczne komponenty o kolorach i rolach wskazanych przez użytkownika; kilku kandydatów blokuje automatyczny wybór.

## Integracja, prywatność i zasady gry

Aplikacja korzysta z obrazu widocznego okna i ręcznych danych. Nie odczytuje pamięci Roblox, nie wstrzykuje kodu, nie modyfikuje klienta i nie wysyła wejścia do gry. Nie steruje pojazdem ani działem i nie strzela. Nie wykrywa ani nie śledzi przeciwników poza markerami, które użytkownik sam wskazał na mapie. Nie rysuje znaczników na celach w widoku 3D i nie ma aim-assist. Korekta „PODNIEŚ/W PRAWO” jest tekstem; działo ustawia gracz.

Skróty Ctrl+Shift+T/G odczytują tylko pozycję kursora i klatkę okna; nic nie klikają. Numpad +/−/0 w trybie „Czołg — szybki” zmienia wyłącznie preset dystansu w aplikacji. Ten skrót globalny przechwytuje klawisze, dopóki tryb jest włączony.

Klatki analizy pozostają w pamięci. Wyjątek stanowi nagrywanie testowe: wymaga zaznaczenia zgody przed każdym nagraniem, zapisuje klatki wyłącznie do katalogu wskazanego przez użytkownika i ma limity klatek, rozmiaru i czasu. Podczas nagrywania widoczny jest wskaźnik. Nic nie jest wysyłane. [Nagrywanie](capture.md#nagrywanie-testowe-aplikacja).

Stan zasad (odczyt 15–16 października 2026):

- [Roblox Community Standards](https://about.roblox.com/community-standards), sekcja „Cheating and Scams”, zakazuje exploitów i narzędzi dających nieuczciwą przewagę.
- [Cheating and Exploiting](https://en.help.roblox.com/hc/en-us/articles/203312450) to artykuł pomocy Roblox o oszustwach i exploitach.
- [Anti-cheat Messages](https://en.help.roblox.com/hc/en-us/articles/24275616578708) (aktualizacja 13 października 2026) dotyczy modyfikacji klienta i ingerencji w proces. Dla OBS zaleca przechwytywanie okna lub pulpitu. Ta wskazówka dotyczy przechwytywania i nie jest zgodą na nakładkę kalkulacyjną.
- Kalkulator i nakładkę publikuje organizacja twórców (Eagle Studios / grand-hawk). Nie jest to zgoda na tę aplikację.
- Regulaminu serwera Discord MTC nie odczytano. Twierdzenie NamuWiki o zakazie programów zewnętrznych pozostaje niezweryfikowane.

Nie uzyskano stanowiska twórców MTC ani Roblox wobec tej nakładki. Projekt **nie deklaruje zgodności z zasadami ani braku ryzyka blokady**. Ocena i odpowiedzialność za użycie podczas rozgrywki należą do użytkownika.

## Stan walidacji i ryzyka

Dokładne liczby testów, wyniki smoke testów gotowego EXE i sumy SHA256 zawiera [raport testów](test-report-v0.5.0.md). [Procedura Windows](windows-validation.md).

| Ryzyko | Zachowanie w 0.5 | Pozostała walidacja |
|---|---|---|
| Stałe broni | Gate: lokalna kalibracja lub potwierdzone dane eksperymentalne; strzały kontrolne | Trafienia, czas lotu i opad w aktualnym MTC |
| Panel broni i mapa M | 8 paneli i mapa Zone 11 z prawdziwej gry; stabilizacja dwóch klatek; wiek 2 s | Inne rozdzielczości, UI scale, języki i bronie; konwencja AZIMUTH |
| Skala i widok mapy | Siatka 9×9 z „Grid size”, katalog 25 map, matcher z weryfikacją | Aktualność tekstur, mapy spoza katalogu, minimapa |
| Kolory i tożsamość celu | Role wskazane przez użytkownika, blokada wielu kandydatów | Rzeczywiste ikony i nakładanie |
| Zasoby i renderowanie | WGC z ograniczonym readbackiem, jedna najnowsza klatka, jawny GDI | Długotrwały benchmark w rozgrywce |
| Opór / napęd / guided | Beta z jawnymi współczynnikami; guided zablokowany | Pomiary w grze |
| DPI i monitory | Fizyczny crop DPI, emulacja 100–200% | Rzeczywiste monitory i przenoszenie okien |
| Skróty i instalacja | Rejestracja i stan natywny, audyt payloadu instalatora | Fizyczne klawisze, test instalacji, podpis cyfrowy |
| Zasady MTC/Roblox | Brak deklaracji zgodności | Stanowisko twórców MTC, regulamin Discord |
