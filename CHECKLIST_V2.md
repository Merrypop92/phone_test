# Checklist PCB V2 - DocPhone

Corrections et ajouts issus des tests de la carte V1 (novembre 2026) et de la
lecture du schema / PCB EasyEDA Pro (ProDoc_Schematic1 / ProDoc_PCB2 du
2026-08-14). Cocher au fur et a mesure de la conception de la V2.

Legende : **[BUG]** erreur sur la V1 - **[MANQUE]** fonction absente -
**[AMELIO]** amelioration conseillee - **[INFO]** a garder en tete.


## 0. Contrainte n°1 : les broches de l'ESP32 sont presque toutes prises

Etat V1 (module ESP32-WROVER) :

| GPIO | Usage V1 | Remarque |
|---|---|---|
| 1 / 3 | Console UART0 (CH340C) | |
| 2, 5, 12, 13 | E-paper DC, BUSY, RST, CS | 2 et 12 sont des broches de demarrage |
| 18, 19, 23 | SPI SCLK, MISO, MOSI | partage e-paper / carte SD |
| 4 | SD_CS | |
| 14 | LED | |
| 15 | Bouton home | broche de demarrage (MTDO) |
| 21, 22 | I2C SDA / SCL | TCA8418 (a supprimer), futur tactile |
| 25 | KDB_INT (TCA8418) | a reutiliser pour l'INT du tactile |
| 26, 27 | UART1 module LTE | via TXB0102 (1,8 V <-> 3,3 V) |
| 32 | PWRKEY LTE (via Q5) | |
| 33 | Vibreur (via Q8) | |
| 39 | Detection USB (pont diviseur +5V) | entree seule |
| **34, 35, 36** | **libres** | **entree seule, sans pull-up interne** |
| 0 | IO0 (auto-reset) | broche de demarrage, a eviter |
| 16, 17 | - | **inutilisables : PSRAM interne du WROVER** |

Besoins V2 identifies ci-dessous : INT + RST tactile, DTR + RI LTE, CHRG +
STDBY TP4056, mesure VBATT, volume + / -, enable ampli audio, INT IMU, LED de
charge... => **ca ne rentre pas**.

- [ ] **[AMELIO] Ajouter un expandeur de GPIO I2C** (ex. TCA6408 8 bits ou
      PCA9555 16 bits, avec sortie INT) sur le bus I2C existant pour les
      signaux lents : boutons de volume, RST tactile, enable ampli, LED de
      charge, CHRG/STDBY... Garder les GPIO directs pour ce qui doit reveiller
      l'ESP32 du sommeil ou etre rapide (INT tactile, RI du LTE).
- [ ] [INFO] Reveil depuis le deep sleep : seulement les GPIO "RTC"
      (0, 2, 4, 12-15, 25-27, 32-39). Mettre RI (LTE) et INT (tactile ou
      expandeur) sur des broches RTC.
- [ ] [INFO] L'ADC2 (GPIO 0, 2, 4, 12-15, 25-27) est inutilisable quand le
      WiFi est actif : toute mesure analogique (VBATT...) doit aller sur
      l'ADC1 (GPIO 32-39).
- [ ] [INFO] GPIO12 (RST e-paper) est une broche de demarrage : s'il est tire
      a l'etat haut au demarrage, la flash/PSRAM passe en 1,8 V et le module
      ne demarre plus. Ne rien mettre qui le tire au 3V3 au reset.


## 1. Module ESP32

- [ ] **[BUG] Symbole du schema** : indique ESP32-WROVER-B (antenne gravee),
      mais le module soude est un **WROVER-IE** (connecteur u.FL, pas
      d'antenne). Mettre le schema et la nomenclature en accord.
- [ ] [MANQUE] Antenne WiFi/BLE 2,4 GHz sur u.FL (IPEX MHF1) : prevoir son
      emplacement dans le boitier, loin de l'antenne LTE et des plans de
      masse. Valide en V1 avec une antenne adhesive 2,4/5 GHz : 19 reseaux,
      -38 dBm ; BLE : -46 dBm.
- [ ] [INFO] Valide en V1 : PSRAM (4 Mo adressables, 0 erreur), rail 3V3
      stable pendant les scans WiFi et BLE (detecteur de brownout actif).
- [ ] [AMELIO] RESET1 (bouton EN) non soude en V1 : le garder dans la
      nomenclature V2.


## 2. Module LTE SIMCom A7682E

- [ ] **[MANQUE] Cabler DTR** (broche 6) a une sortie de l'ESP32 (ou de
      l'expandeur) : necessaire pour la veille du module (`AT+CSCLK=1`), gros
      poste de consommation.
- [ ] **[MANQUE] Cabler RI** (broche 7) a une entree RTC de l'ESP32 : reveil
      de l'ESP32 sur appel ou SMS entrant (aujourd'hui tout passe par la
      lecture de l'UART, impossible en sommeil).
- [ ] [INFO] DTR et RI sont dans le domaine 1,8 V du module : passer du
      TXB0102 (2 voies) a un **TXB0104** (4 voies) ou equivalent.
- [ ] [AMELIO] SIM1_DET (broche 14) non cablee : si le lecteur KH-SIM1616 a
      un contact de detection, le relier pour activer la detection a chaud
      (aujourd'hui il faut `AT+CRESET` apres avoir change de SIM).
- [ ] [AMELIO] Ajouter un condensateur de decouplage sur SIM_VDD pres du
      lecteur (100 nF, voire 1 uF en plus, comme sur le schema de reference
      SIMCom) : en V1 il n'y a que la protection ESD (D2).
- [ ] **[MANQUE] Antenne LTE large bande** (~698-960 MHz et 1710-2690 MHz).
      Une antenne WiFi capte mal les bandes basses (800 MHz / B20 et 700 MHz
      / B28, celles de Bouygues qui portent loin). Verifier l'adaptation
      d'impedance et l'empreinte u.FL (U2).
- [ ] [INFO] Valide en V1 : allumage PWRKEY, SIM, reseau 4G Bouygues, heure
      reseau (NITZ), SMS emission/reception, -67 dBm.
- [ ] [INFO] Le module est sur VBATT : il reste allume quand l'ESP32
      redemarre. Pics de ~2 A en emission : garder des condensateurs de
      reservoir genereux pres de VBAT (34/35).


## 3. Alimentation, batterie, charge

- [ ] **[BUG] GPIO39 "IS_CHARGING" n'est pas l'etat de charge** : c'est un
      pont diviseur R4/R3 (10k/10k) sur le +5V USB, donc la **presence de
      l'USB** (valide en V1). Renommer le net en USB_DETECT.
- [ ] **[MANQUE] Etat de charge reel** : relier CHRG et STDBY du TP4056
      (sorties a drain ouvert) a deux entrees, avec pull-ups vers 3V3
      (GPIO34/35 conviennent, ou l'expandeur). Possibilite de garder la LED
      de charge en parallele.
- [ ] **[MANQUE] Mesure de la tension batterie** : aujourd'hui la seule
      mesure est `AT+CBC` via le module LTE. Ajouter un pont diviseur VBATT
      vers une entree ADC1 (ex. GPIO36), avec de fortes valeurs pour limiter
      la fuite (ex. 2 x 220 kOhm + 100 nF), ou mieux une **jauge de batterie
      I2C** (ex. MAX17048) qui donne directement le pourcentage.
- [ ] [AMELIO] **Pas de power-path sur le TP4056** : en charge, la carte et
      le module LTE (pics de 2 A) tirent sur la batterie en meme temps que le
      chargeur la remplit, ce qui fausse la fin de charge. Envisager un
      chargeur avec power-path (ex. MCP73871, BQ24074) ou un montage de
      partage de charge (MOSFET P + diode Schottky).
- [ ] [AMELIO] Prevoir de **mesurer la consommation** : un cavalier ou une
      resistance 0 Ohm dans la ligne VBATT (et/ou 3V3), pour inserer un
      multimetre, et des points de test.
- [ ] [INFO] Valide en V1 : protection DW01A + FS8205, TPS63025 (3V3_SYS),
      LDO ME6219 (3V3_EPD), detecteur de brownout (seuil ~2,43 V) actif.


## 4. Audio

- [ ] [INFO] SPK1P/SPK1N du A7682E est une sortie classe AB pour **ecouteur**
      (~32 Ohm, ~37 mW), via C11/C19 vers des pastilles nues HP+/HP-.
      Suffisant pour un combine a l'oreille (ex. Soberton RC-1504).
- [ ] [MANQUE] Pour un **haut-parleur mains libres / sonnerie forte** :
      ajouter un ampli classe D (ex. NS4150, PAM8302) avec une broche
      d'enable, et un haut-parleur 8 Ohm.
- [ ] [AMELIO] Connecteurs ou pastilles dediees, reperees, pour le micro,
      l'ecouteur et le haut-parleur.
- [ ] [INFO] Micro : empreinte electret GMI4015P-2C-30DB (4015, traversant),
      polarisation et filtrage presents (R21, C24-C26). Non teste (piece en
      commande).
- [ ] [INFO] L'"ampli casque" de la liste d'origine n'existe pas sur ce PCB.


## 5. Vibreur

- [ ] **[BUG] Pas de diode de roue libre** aux bornes du moteur : seulement
      C18 (4,7 uF) entre le drain de Q8 et le 3V3. Ajouter une diode (BAT54
      ou 1N4148) en inverse sur le moteur.
- [ ] [AMELIO] Q8 = 2N7002 : Rds(on) de quelques ohms a 3,3 V de grille,
      juste pour un moteur de ~80 mA. Prendre un MOSFET "logique" (AO3400,
      SI2302).
- [ ] [AMELIO] Pastilles ou connecteur dedies pour le moteur (aucun en V1 :
      il faudrait souder sur C18).


## 6. Entrees : clavier, tactile, boutons

- [ ] **[A SUPPRIMER] Matrice de boutons TCA8418** (U11 et ses composants) :
      remplacee par l'ecran tactile. Pour memoire, en V1 sa broche VCC n'etait
      pas reliee au 3V3 et R_9 etait en serie sur INT# au lieu d'un pull-up.
- [ ] **[MANQUE] Ecran tactile** (GDEQ0426T82-T01C, controleur **FT6336U**) :
      connecteur FPC de la dalle tactile, I2C sur SDA=21 / SCL=22 (adresse
      0x38, compatible avec les pull-ups 4,7 kOhm existants), INT (actif bas,
      reprendre GPIO25 = broche RTC, donc reveil possible), RST (sortie :
      expandeur ou GPIO).
- [ ] [MANQUE] Boutons de volume + / - (entrees : GPIO34/35/36 avec pull-ups
      externes, ou expandeur).
- [ ] [INFO] Valide en V1 : bouton home (GPIO15, pull-up interne, appui =
      niveau bas). GPIO15 maintenu appuye pendant un reset coupe seulement les
      messages de la ROM.


## 7. Autres ajouts prevus

- [ ] [MANQUE] Centrale inertielle (accelerometre + gyroscope) en I2C, INT
      sur une broche RTC si on veut reveiller sur mouvement.
- [ ] [MANQUE] LED de charge (ou piloter la LED existante selon CHRG/STDBY).
- [ ] [MANQUE] RTC externe sur pile (ex. DS3231) : garder l'heure sans
      reseau et telephone eteint (aujourd'hui l'heure vient du reseau LTE).
- [ ] [MANQUE] GPS (le A7682E n'en a pas) : module separe ou variante du
      module LTE avec GNSS (verifier la disponibilite chez SIMCom).
- [ ] [INFO] Carte microSD (P10, SPI partage avec l'e-paper, CS=GPIO4) :
      non testee (carte en commande).
- [ ] [INFO] Pas de flash externe (W25Q16) ni de puce de police (GT30L32S4W)
      a prevoir : le WROVER a 16 Mo de flash, et les polices sont dans le
      firmware.
