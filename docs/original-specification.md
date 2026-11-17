# MTC Tactical Assistant — pakiet przekazania do ChatGPT Work

**Projekt:** samodzielna aplikacja Windows z nakładką na grę Roblox **Multicrew Tank Combat**, szczególnie epoka **Modern**.
**Stan:** specyfikacja startowa; implementacja jeszcze nie została wykonana. Nie traktować danych liczbowych ani modeli balistycznych jako zweryfikowanych.
**Cel użytkownika:** możliwie pełne, automatyczne obliczenia w czasie rzeczywistym dla wszystkich typów uzbrojenia, bez konieczności ręcznego wpisywania współrzędnych, tam gdzie da się je wiarygodnie odczytać z obrazu.

## 1. Kontekst rozmowy

- Użytkownik najczęściej gra jako **celowniczy** w trybie **Modern**, ale lata także **śmigłowcami** i korzysta z **artylerii rakietowej**.
- Na dołączonych zdjęciach widać: śmigłowiec nad mapą leśno-polną oraz wyrzutnię przypominającą **BM-21 Grad**. Identyfikację modelu, mapy i mechanik należy zweryfikować, a nie uznawać za pewnik.
- Użytkownik chce nakładki działającej **na żywo**, która identyfikuje dostępne dane z ekranu, oblicza parametry, wyświetla podpowiedzi i wspiera różne profile broni.
- Referencje: `references/smiglowiec_rozgrywka.jpg`, `references/bm21_grad_rozgrywka.jpg`.

## 2. Efekt końcowy

Działające lokalnie narzędzie (instalator lub `.exe`) dla Windows 10/11 z:

1. Przezroczystą nakładką **always on top**, przełącznikiem click-through, skrótem ukrywania, skalowaniem DPI, możliwością przesuwania panelu.
2. Odczytem obrazu gry przez wspierane publiczne API przechwytywania okien/ekranu (np. Windows Graphics Capture), bez modyfikowania procesu gry.
3. Rozpoznawaniem HUD, widocznej minimapy, kompasu, liczb, nazw uzbrojenia, ikon i widocznych oznaczeń przy użyciu OCR/CV; indywidualna kalibracja dla rozdzielczości i UI.
4. Profilem **Manual / Assisted / Automatic**: ręczny fallback, półautomatyczne wskazanie celu na mapie i automatyczny odczyt tylko gdy pomiary mają dostateczną pewność.
5. Modułem map i współrzędnych: rozpoznawanie mapy (z ręcznym wyborem gdy nierozpoznana), siatka sektorów, kalibracja układu współrzędnych, wskazanie własnej pozycji i celu z obrazu oraz korekty na podstawie punktów referencyjnych.
6. Silnikiem balistycznym: dystans, azymut, kąt podniesienia, przewidywany czas lotu, rozrzut, punkt/obszar trafienia oraz niepewność pomiaru (jeśli dane i model pozwalają).
7. Modułami osobno dla artylerii lufowej, artylerii rakietowej (BM-21/MLRS), czołgów, działek śmigłowcowych, rakiet śmigłowcowych i uzbrojenia przeciwlotniczego. Zakres i dostępne funkcje powinny wynikać ze zweryfikowanych profili broni.
8. Profilem uzbrojenia jako rozszerzalnymi danymi (JSON), z wersją gry, źródłem/pochodzeniem parametrów, walidacją i możliwością ręcznej kalibracji.
9. Odczytem widocznych na ekranie markerów oraz możliwością ręcznego oznaczenia celu. **Nie obiecywać wykrywania przeciwników ukrytych przed użytkownikiem.**
10. Lokalnym zapisem ustawień, logiem kalibracji i testowymi nagraniami (nagrania tylko za zgodą użytkownika).
11. Instalacją, README, testami automatycznymi, instrukcją obsługi i planem rozwoju.

## 3. Najważniejsze wymogi jakościowe

- Nie podawać dokładnego azymutu lub elewacji, gdy brakuje poprawnej skali mapy, pozycji, profilu pocisku lub stałych balistycznych; w UI pokazać `brak danych / wymagane ustawienie`.
- Każda predykcja ma wskaźnik pewności i listę założeń, gdy jest to przydatne.
- Rozróżniać: dane odczytane z UI, dane wprowadzone przez gracza, dane wyliczone, dane jedynie szacowane.
- Zachować warstwę obliczeń niezależną od OCR/UI, aby można ją było testować bez uruchamiania Roblox.
- Nie zapisywać ani nie wysyłać klatek ekranu domyślnie; przetwarzanie offline jest standardem.
- Zapewnić kontrolę wydajności CPU/GPU, kolejkę klatek z odrzucaniem przeterminowanych pomiarów, ograniczenie FPS OCR i bezpieczny stan przy utracie obrazu.
- Tryb pełnoekranowy może wymagać `borderless windowed` — dokumentacja ma zawierać ograniczenia.

## 4. Granice integracji / uczciwe działanie

Aplikacja powinna być **nieinwazyjnym narzędziem pomocniczym**. Bez odczytu pamięci gry, wstrzykiwania DLL, modyfikowania pakietów, exploitów, obchodzenia zabezpieczeń, automatyzacji strzelania i sterowania pojazdem. Obraz i dane pochodzą tylko z tego, co gracz normalnie widzi, albo z ręcznych danych. Należy zbadać warunki Roblox oraz reguły MTC dotyczące nakładek; nawet takiej aplikacji **nie można obiecać jako zgodnej z regulaminem lub wolnej od ryzyka blokady**. W przypadku niepewności oddzielić tryb offline/treningowy od trybu na żywo.

## 5. Architektura proponowana do oceny przez Work

- **UI/overlay:** Tauri 2 + React + TypeScript (lub technologia o lepiej udokumentowanej zgodności z transparentnym click-through pod Windowsem). Interfejs po polsku; opcjonalny angielski.
- **Warstwa natywna:** Rust, przechwytywanie obrazu Windows Graphics Capture, ekstrakcja ROI, warstwa OCR/CV (na początek OCR systemowy / OpenCV zależnie od benchmarku).
- **Core ballistics:** Rust jako czysta biblioteka deterministycznych obliczeń z testami jednostkowymi; osobna warstwa profili uzbrojenia i kalibracji; współdzielona przez UI jako komendy Tauri.
- **Map layer:** map-specific configuration, transformacja współrzędnych, wizualna kalibracja dwóch lub więcej punktów, osobne dane skali mapy.
- **Persistence:** profile JSON + wersjonowane migracje ustawień; bez chmury na start.
- **Diagnostics:** diagnostyka OCR, wizualizacja ROI i wskaźnik jakości, log bez ujawniania wrażliwego obrazu.

Można wykorzystać istniejące publiczne projekty jako materiał referencyjny, ale **przed kopiowaniem kodu trzeba potwierdzić repozytorium i licencję** (np. projekt kalkulatora artylerii `grand-hawk/artillery-calculator` został wspomniany w rozmowie, lecz Work ma sam zweryfikować dostępność, licencję oraz mechanikę).

## 6. Sugerowana struktura katalogów

```text
mtc-tactical-assistant/
  README.md
  docs/
    requirements.md
    architecture.md
    limitations-and-compliance.md
    weapon-profiles.md
    calibration.md
  apps/
    desktop/                 # Tauri + React + TypeScript
  crates/
    ballistics/              # modele i solvery
    capture/                 # przechwytywanie obrazu
    vision/                  # wykrywanie ROI i OCR
    maps/                    # układy współrzędnych
    profiles/                # zarządzanie profilami
  data/
    weapons/
    maps/
  tests/
    fixtures/
  scripts/
```

## 7. Dokładny podział funkcji według rodzaju uzbrojenia

**Artyleria lufowa:** wybór broni/amunicji, znana pozycja działa + celu, solwer elewacji i azymutu, podgląd wariantów trajektorii (jeśli gra na to pozwala), korekta trafienia i historia salw.

**Artyleria rakietowa:** BM-21/MLRS, prognoza środka salwy i rozrzutu, interaktywny sektor obszaru trafień, rozdzielenie parametrów pojedynczej rakiety i całej salwy.

**Czołgi:** zależnie od modelu pocisku oszacowanie opadu oraz wyprzedzenia z widocznych danych; narzędzia treningowe rozpoznawania podstawowych typów pocisków; nie przedstawiać jako nieomylnego wskazania słabych punktów.

**Śmigłowce:** pomocnik lotu (dostępny kompas, wysokość i prędkość z HUD), wspomaganie obliczeń dla działek i uzbrojenia kierowanego tylko tam, gdzie zasady sterowania są znane; rozróżniać mechanikę rakiet naprowadzanych od pocisków balistycznych.

**Przeciwlotnicze:** kalkulator wyprzedzenia ruchomego celu na podstawie rzeczywiście obserwowanych pozycji, czasu i dostępnej prędkości pocisku; ostrzeżenie o brakujących danych.

**Współpraca:** tryb współdzielonych oznaczeń (najpierw lokalny eksport/import, opcjonalnie proste LAN później), bez ingerencji w komunikację serwera gry.

## 8. Automatyczne rozpoznawanie i wiarygodność

System ma działać etapowo:

1. Rozpoznanie, czy otwarto mapę czy widok z pojazdu, i podstawowych obszarów HUD.
2. Odczyt napisów/liczb z wyznaczonych obszarów ekranu + poprawność/pewność OCR.
3. Rozpoznanie wariantu pojazdu **z jawnego napisu w UI**, a gdy brak nazwy: manualny wybór; model komputerowy może być dodany później.
4. Kalibracja mapy, osi i skali na podstawie informacji z gry lub punktów referencyjnych; żadnego arbitralnego przypisywania współrzędnych na podstawie fotografii ekranu.
5. Obliczenia aktualizowane tylko gdy pomiary są świeże i kompletne; pasek ostrzeżeń.

**Nie zakładać dostępności kompletnej telemetrii, API Roblox/MTC ani współrzędnych świata.** To zasadnicze ryzyko projektu.

## 9. Etapy realizacji i testy akceptacyjne

**Etap 0 – Discovery (krótki):** zweryfikuj aktualną grę, HUD, rodzaje broni, regulamin, aktualną platformę, dostępne dane i referencyjne kalkulatory. Udokumentuj, czego nie da się odczytać.

**Etap 1 – Uruchamialny MVP:** aplikacja Windows z overlay, obsługa skrótów, ustawienia, wybór BM-21 i innych profili demonstracyjnych, kalkulator azymutu/dystansu na ręcznie oznaczonych punktach mapy, testy solvera. Uruchom build i dostarcz wyniki testów.

**Etap 2 – Automatyczny odczyt:** przechwytywanie okna gry, wykrycie HUD, OCR kompasu i dostępnych wartości, debug wizualny, test na dostarczonych dwóch zdjęciach; UI musi rozróżnić obserwacje od zgadywania.

**Etap 3 – Mapy i kalibracja:** import map, konfigurator punktów, rozpoznawanie map, ręczne kliknięcie celu, korekta strzałów, walidacja na nagraniach.

**Etap 4 – Wiele rodzajów uzbrojenia:** nowe profile i osobne moduły artylerii lufowej, rakietowej, czołgów, śmigłowców i przeciwlotniczych; testy regresyjne.

**Etap 5 – Stabilizacja i wydanie:** przejrzysty panel, lokalizacja PL, benchmark, diagnostyka, dokumentacja i podpisanie wydania jeśli możliwe.

**Warunek akceptacji etapu 1:** użytkownik może pobrać/uruchomić aplikację, wybrać pojazd i wskazać pozycję działa oraz celu na skonfigurowanej mapie, otrzymując poprawny dystans i azymut, a elewację wyłącznie dla skalibrowanych profili. Testy jednostkowe i README działają.

**Warunek akceptacji etapu 2:** aplikacja poprawnie przechwytuje ekran Windows i interpretuje co najmniej jedną wartość HUD na wiarygodnym obrazie, bez obiecywania automatycznego rozpoznawania całej mapy.

**Warunek akceptacji etapu końcowego:** wszystkie kategorie uzbrojenia mają moduł i stan wsparcia (zweryfikowany / beta / eksperymentalny / brak danych), a nie fikcyjne wyniki.

## 10. Polecenia dla ChatGPT Work

- Nie kończ na samym planie: rozpocznij implementację i testy w workspace.
- Najpierw sprawdź środowisko i zależności; jeśli Windows build nie jest dostępny, przygotuj działającą część platformowo niezależną, opisz blokadę i dostarcz instrukcję kompilacji pod Windows.
- Nie wymyślaj danych balistycznych ani zdolności automatycznego wykrywania; brak danych oznaczaj w UI.
- Zachowuj commitowalny stan plików i klarowny status postępów. Pokazuj, co działa, a co wymaga nagrań/kalibracji.
- Nie wymagaj od użytkownika codziennych decyzji, jeśli można przyjąć rozsądne domyślne rozwiązania; pytaj tylko o blokujące informacje.
- Na końcu każdego etapu przygotuj konkretny artefakt, testy i instrukcje dalszych działań.
