# Weryfikacja okien Windows i pakietu instalatora

Test bazowy wykonano 2026-10-13 na portable `release/MTC-Tactical-Assistant-0.1.0.exe`. Artefakty: `tests/windows-output/v0.1.0/window-controls-report.json` oraz niezależny przebieg CLI `tests/windows-output/v0.1.0-cli/window-controls-report.json`. Okna testowe, profil WebView i ustawienia użytkownika były oddzielone od sesji aplikacji. CDP używał portu 9224. Po testach zamknięto wyłącznie procesy uruchomione przez test.

## Ponowiony test wersji 0.2.0

Po zwolnieniu komputera przez użytkownika wykonano test na `.exe` 0.2.0. Raport: `tests/windows-output/v0.2.0/window-controls-report.json`. **Pięć rzeczywistych wejść przeszło:** O pokazuje/ukrywa nakładkę, I włącza/wyłącza click-through, M zachowuje panel na pierwszym planie. Każdy klawisz wysłano po sprawdzeniu PID i foreground głównego panelu. Nie wystąpiła blokada wejścia użytkownika.

M był testowany z już aktywnym głównym panelem; nie potwierdza to przywracania po minimalizacji. Pokazanie nakładki przez O przełączyło foreground z panelu na własne okno nakładki. Ten wynik przekazano do poprawki; przed następnym klawiszem ponownie aktywowano główny panel. Przy zmianie implementacji pokazania nakładki należy powtórzyć fizyczny test O na nowym `.exe`.

Pozostałe wyniki 0.2.0: 5/5 natywnych przejść widoczności/click-through zgodnych z flagami Windows, rzeczywiste DPI 125%, 16 przypadków emulacji. Nakładka ma teraz 390×340 logicznych px i mieści pusty stan przy 100/125/150/200%. Przy stałych 390×340 fizycznych px pionowy overflow nadal występuje przy 125/150/200%; jest to osobny test ograniczonej przestrzeni, nie dowód błędu natywnej zmiany DPI.

Instalator 0.2.0 zawiera pełną licencję Astrid w `licenses/mtc-community-MIT.txt`, SHA256 `23c32fc597a95f4fb87a03405e9c5f6c6bc8c3441605f3896dc213fc12560afe`, identyczny z plikiem źródłowym. Payload jest identyczny z portable po uwzględnieniu markera typu pakietu Tauri. Instalatora nie uruchamiano. Proces testowy i port 9224 zwolniono przed dalszymi testami aplikacji.

W przebudowanej wersji 0.2.0 po pierwszej poprawce focus (`tests/windows-output/v0.2.0-final/window-controls-report.json`) pokazanie O zachowało aktywny panel. Wykryto jednak regresję: włączenie I ukrywało nakładkę zamiast zachować jej widoczność. Przebieg przerwano, zapisując jeden niezaliczony przypadek i cztery rzeczywiste wejścia. Ten plik jest dowodem wykrycia błędu, nie raportem akceptacji końcowej wersji. Test od tej chwili sprawdza także ustaloną widoczność 250 ms po I.

Kolejna poprawka synchronizuje widoczność przez Tauri i utrzymuje nakładkę nieaktywną przez `set_focusable(false)`. Fizyczny test nowego debug `.exe` zatrzymał użytkownik klawiszem Esc przed wysłaniem jakiegokolwiek skrótu. Zakończono własny proces i nie ponawiano Computer Use. Wyniku wcześniejszego `.exe` nie przenosimy jako akceptacji nowego pliku. Przywracanie zminimalizowanego panelu przez M pozostaje niepotwierdzone; przygotowany wariant testu dopuszcza jedynie znany, lokalnie zbudowany `hud_fixture.exe`, jego dokładną ścieżkę/PID/HWND i świeżą obserwację.

Końcowa weryfikacja przez API przeszła 2026-10-13 o 22:51 czasu Warszawy. Raport: `tests/windows-output/v0.2.0-api-final/window-controls-report.json`. SHA256 testowanego portable: `0536aeb1ccf45513b9a8b27a47dc7d13124b3b138118f63b43cbc4fba22384f8`.

Wszystkie pięć natywnych przejść stanu przeszło po sprawdzeniu ustalonego stanu: pokazanie/ukrycie, topmost i włączenie/wyłączenie click-through są zgodne z rzeczywistymi flagami Windows. Regresja widoczności po I nie wystąpiła w tych wywołaniach API. Trzy globalne skróty są zarejestrowane. `physicalHotkeysTested: 0`: końcowego pliku nie sprawdzano fizycznymi klawiszami po zatrzymaniu Computer Use przez Esc.

Wykonano 16 emulacji. Główny panel nie miał poziomego overflow ani obciętych poziomo kontrolek. Nakładka przy logicznych 390×340 px mieści pusty stan w 100/125/150/200%; test ograniczonej stałej przestrzeni fizycznej nadal ujawnia pionowy overflow przy 125/150/200%. Rzeczywiste DPI wynosi 125%; przed emulacją WebView panelu miał 1220×844 px, a nakładki 391×340 px. Podane natywne wymiary raportu są fizyczne — inspektor ustawia DPI awareness wyłącznie swojego chwilowego wątku, bez zmian ustawień Windows.

Końcowy NSIS zawiera pełną licencję MIT Astrid z oczekiwanym SHA256. Jego payload odpowiada portable po uwzględnieniu markera Tauri; instalatora nie wykonano. CLI zakończył się kodem 0, zamknął własny proces, a brak procesu testowego i listenera portu 9224 potwierdzono odczytem Windows. Fizyczne przywracanie zminimalizowanego panelu przez M oraz natywne przejścia DPI między monitorami pozostają niepotwierdzone.

```powershell
node scripts/test-window-controls.mjs --exe release/MTC-Tactical-Assistant-0.2.0.exe --installer "release/MTC Tactical Assistant_0.2.0_x64-setup.exe" --output-dir tests/windows-output/v0.2.0-api-final --port 9224 --overlay-height 340 --physical-note "Computer Use zatrzymany przez użytkownika klawiszem Esc; w tym przebiegu wyłącznie testy przez API, bez fizycznych klawiszy."
```

Nie uruchamiaj tego polecenia jednocześnie z inną instancją MTC Tactical Assistant: globalne skróty muszą być dostępne dla testowanego procesu. Audyt instalatora pozostaje rozpakowaniem, bez wykonania NSIS i zapisów rejestru.

## Rzeczywiste wyniki wersji 0.1.0

| Zakres | Wynik i granice |
| --- | --- |
| Rejestracja Ctrl+Shift+O / I / M | 3/3 zarejestrowane przez natywny plugin, zgodnie z diagnostyką `.exe` |
| Faktyczne wysłanie klawiszy | **Niepotwierdzone.** Computer Use wstrzymał próbę z powodu wykrycia wejścia użytkownika; odczyt foreground wskazał okno Roblox. Nie wykonywano kolejnych prób ani nie wysyłano klawiszy do gry |
| Widoczność/click-through | 5/5 przejść przez API własnej aplikacji. `IsWindowVisible` i `WS_EX_TRANSPARENT` odpowiadały deklarowanemu stanowi; `WS_EX_TOPMOST` pozostawał włączony |
| Rzeczywiste DPI | `GetDpiForWindow = 120`, `dpiScale = 1.25`, czyli 125%. Ustawień monitora nie zmieniano |
| Główny panel, emulacja 100/125/150/200% | Brak poziomego overflow i obciętych poziomo przycisków/inputów. Strona wymaga pionowego przewijania |
| Nakładka 390×310 logicznych px, emulacja 100/125/150/200% | Pusty stan nakładki mieści się w każdej z czterech emulacji |
| Nakładka przy stałych 390×310 fizycznych px | Test przeciążeniowy ujawnił pionowy overflow przy 125/150/200%: odpowiednio logiczne 312×248, 260×207 i 195×155 px |
| NSIS 0.1.0 | Rozpakowano bez wykonania. Portable i payload mają poprawny PE x64. Różnica plików to wyłącznie trzy bajty markera Tauri `BUNDLE_TYPE_VAR_UNK` → `BUNDLE_TYPE_VAR_NSS` |
| Instalacja do katalogu izolowanego | **Niewykonana.** Skrypt NSIS zapisuje HKCU i wpisy Uninstall przez SHCTX; osobny katalog nie zapewnia izolacji rejestru |

Przeprowadzono 16 przypadków emulacji: dwa widoki × dwa scenariusze rozmiaru × cztery skale. Scenariusz stałego rozmiaru logicznego odpowiada oknu zachowującemu jego logiczne wymiary przy zmianie DPI. Scenariusz stałego rozmiaru fizycznego bada ograniczoną przestrzeń ekranu. Dla panelu głównego przy 150/200% rozmiary w tym drugim scenariuszu spadają poniżej skonfigurowanego minimum 820 px; raport oznacza je jako test przeciążeniowy.

Odczyt bitu `WS_CAPTION` nie jest samodzielnym dowodem widocznej dekoracji DWM/Tauri. Także brak `WS_EX_LAYERED` w trybie edycji nie jest dowodem braku przezroczystości DirectComposition. Test nie zastępuje oględzin przezroczystości nad grą, kliknięcia przez nakładkę ani obsługi `WM_DPICHANGED` podczas przenoszenia pomiędzy rzeczywistymi monitorami.

## Powtarzalny test bez fizycznych klawiszy

Wymagania: Windows, Python 3.10+, Node 20+, zależności npm aplikacji (`playwright`) i `7z`/NanaZip dla audytu instalatora. Uruchom w katalogu głównym:

```powershell
node scripts/test-window-controls.mjs --exe release/MTC-Tactical-Assistant-0.1.0.exe --installer "release/MTC Tactical Assistant_0.1.0_x64-setup.exe" --output-dir tests/windows-output/v0.1.0-cli --port 9224
```

Polecenie sprawdzono: exit 0, 5 natywnych przypadków stanu, 16 emulacji, 0 fizycznych skrótów, instalator niewykonany. Skrypt odmawia działania, jeśli port CDP jest zajęty. Zamyka własny proces po teście, weryfikując jego pełną ścieżkę. Wydanie 0.2.0 testuje się tym samym poleceniem, podając nowe ścieżki `.exe` i instalatora oraz osobny katalog wyników.

Rozmiar nakładki można podać przez `--overlay-height 340` (0.2.0). Dla testu przez `beginWindowTest` użyto `windowSizes: {main: {width: 1220, height: 900, minimumWidth: 820}, overlay: {width: 390, height: 340}}`. Raport zachowuje rzeczywiste rozmiary WebView i DPR przed emulacją, aby odróżnić je od zadanych scenariuszy.

`scripts/test-window-controls.py` odczytuje HWND, PID, foreground, widoczność, minimalizację, style, DPI i wymiary. Nie zawiera SendInput ani zmian DPI/rejestru. Obsługuje również rozpakowanie NSIS, PE/SHA256 i wykrycie pełnej licencji MIT Astrid. Brak licencji społeczności w instalatorze 0.1.0 jest oczekiwany: ta wersja poprzedza import danych. Dla wydania zawierającego dane społeczności pole `communityLicenseIncluded` musi być prawdziwe.

## Prawdziwe skróty z ochroną foreground

Do testu wejścia służą funkcje `observeWindowTest` i `pressWindowTest` z modułu `.mjs`, wywoływane w `node_repl` przez Computer Use `@oai/sky`. Każda akcja musi nastąpić w osobnej komórce po odczytaniu obserwacji. Uruchom własną aplikację z izolowanymi danymi:

```powershell
python scripts/test-window-controls.py launch --exe release/MTC-Tactical-Assistant-0.1.0.exe --output-dir tests/windows-output/physical-keys --port 9224
```

Polecenie zwraca PID. W `node_repl` zainicjuj Computer Use, importuj moduł i otwórz sesję dla dokładnej ścieżki tego `.exe`:

```javascript
globalThis.sky = (await import('@oai/sky')).sky;
globalThis.tests = await import('file:///C:/path/to/mtc-tactical-assistant/scripts/test-window-controls.mjs');
globalThis.session = await tests.beginWindowTest({
  pid: /* PID otrzymany z launch */,
  exe: 'C:\\path\\to\\mtc-tactical-assistant\\release\\MTC-Tactical-Assistant-0.1.0.exe',
  outputDir: 'C:\\path\\to\\mtc-tactical-assistant\\tests\\windows-output\\physical-keys',
  port: 9224,
});
nodeRepl.write(JSON.stringify(await tests.observeWindowTest(session, sky)));
```

Panel musi być aktywnym oknem. Po sprawdzeniu obserwacji wykonaj jedną akcję i odczytaj wynik:

```javascript
nodeRepl.write(JSON.stringify(await tests.pressWindowTest(session, sky, 'O')));
```

Powtórz odczyt i akcję dla O (ukrycie), I (włączenie i wyłączenie click-through), M (panel na wierzchu). Bezpośrednio przed każdym klawiszem skrypt ponownie sprawdza pełną ścieżkę procesu, PID i HWND aktywnego panelu. Jeśli użytkownik zmieni focus, test kończy tę próbę błędem. Nie wolno pomijać tego zabezpieczenia, aby wysłać skrót w oknie gry.

`Ctrl+Shift+M` po zminimalizowaniu/ukryciu panelu pozostaje osobnym testem ręcznym: wymaganie wysyłania automatycznych klawiszy wyłącznie przy aktywnym głównym panelu uniemożliwia taki scenariusz. Nie oznaczaj samej rejestracji skrótu ani wywołania API jako zaliczonego fizycznego testu przywracania.

Zapisz raport przez `finishWindowTest(session)`, a następnie zakończ własny proces:

```powershell
python scripts/test-window-controls.py stop --pid <PID-testu> --exe release/MTC-Tactical-Assistant-0.1.0.exe
```

## Audyt instalatora bez instalacji

```powershell
python scripts/test-window-controls.py payload --installer "release/MTC Tactical Assistant_0.1.0_x64-setup.exe" --exe release/MTC-Tactical-Assistant-0.1.0.exe --output-dir tests/windows-output/payload
```

Polecenie sprawdzono. Rozpakowanie nie wykonuje `[NSIS].nsi` ani `uninstall.exe`. Raport zawiera przykładowe instrukcje zapisu rejestru, maszynę PE, SHA256, porównanie payloadu i pliki licencji. Stub instalatora NSIS może być PE x86 przy właściwym payloadzie x64; to oczekiwana konstrukcja, nie błąd architektury aplikacji.

Emulacja używa [CDP Emulation.setDeviceMetricsOverride](https://chromedevtools.github.io/devtools-protocol/tot/Emulation/#method-setDeviceMetricsOverride), a po każdym widoku przywraca metryki. Screenshoty przedstawiają wyłącznie WebView aplikacji; test nie nagrywa pulpitu ani gry.
