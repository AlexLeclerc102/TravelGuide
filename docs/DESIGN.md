# Spécification de design — « Carnet de route »

Source : canevas de design [TravelGuide – Page voyage : 3 directions](https://claude.ai/artifact/K154H3eEyHZQJXfDkmUBso)
(29 planches, tours 1 à 6). Ce document décrit la direction **retenue** (tour 2, A' « Carnet de
route ») et tous les écrans dessinés ensuite (tours 3 à 6, mobile). Les directions du tour 1
(A « Carnet de voyage », B « Atlas de nuit », C « Boussole ») et D « Néobrutalisme » (tour 2)
sont écartées et ne sont pas décrites ici.

La planche « Feuille de composants » (tour 4) fait référence. En cas de doute sur une valeur,
c'est elle qui tranche, puis les écrans.

## 1. Principes

- Un carnet de route posé sur une carte ancienne : fond sable, surfaces papier, encre bleu nuit.
- Peu d'ingrédients : coins presque droits (2 px), traits fins couleur encre, **aucune ombre**,
  des aplats, trois polices. La hiérarchie vient des traits et des aplats.
- Le vert forêt porte l'action principale ; la moutarde marque ce qui est choisi, planifié ou une zone.
- La couleur n'est jamais le seul signal : une teinte d'étape est toujours doublée d'un nom ou d'un
  numéro, les repères de carte ont des formes différentes, une erreur porte toujours « ! » et un texte.
- Pas d'emoji. Icônes en trait de 1,8 à 2 px.

## 2. Jetons

### 2.1 Couleurs

| Jeton | Valeur | Usage |
|---|---|---|
| `--sable` | `#E8E1D1` | fond de page, fond du panneau autour des onglets |
| `--papier` | `#F6F2E8` | surface du panneau, onglet actif, contrôles de carte |
| `--carte` | `#FBF9F3` | cartes, champs, billets |
| `--encre` | `#1F2A37` | texte, traits, barre haute, bouton sombre |
| `--foret` | `#2F5D50` | bouton principal, liens, catégorie nature |
| `--foret-survol` | `#244A40` | survol du bouton principal |
| `--moutarde` | `#E2B33C` | planifié, sélection, zone d'attache, focus, marque |
| `--surbrillance` | `#F3E3B3` | survol de ligne, avertissement, élément choisi |
| `--onglet` | `#D6CDB6` | onglet inactif, jauge vide |
| `--pointille` | `#A79E86` | séparateurs pointillés |
| `--alerte` | `#8B2E2E` | danger, erreur (réservé aux vrais problèmes) |
| `--alerte-fond` | `#F6E3DF` | fond des bandeaux d'erreur |
| `--texte2` | `#4D5563` | texte secondaire (≈ 6:1 sur papier) |
| `--texte-corps` | `#2B3544` / `#3D4654` | texte courant légèrement adouci (listes, sous-titres) |
| `--mer` | `#BFD0CE` | eau sur la carte, fond de la zone carte |
| `--terre` | `#EFE8D2` | terre sur la carte |
| `--relief` | `#8DA68A` | hachures des parcs et reliefs |
| `--en-ligne` | `#2F7D57` | présence en direct (pastille carrée) |

Barre haute (sur encre) : texte `#EDE7D8`, texte secondaire `#BFC4CC`, statut serveur `#6FBF8E`,
marque et lien « Se déconnecter » en moutarde.

**Couleurs d'étapes**, dans l'ordre : `#E2B33C`, `#2F5D50`, `#6B8FB5`, `#8A6D9E`, `#7A8F3F`,
puis `#A79E86`, `#C4B27A`, `#8FA7B5` au-delà de 5 étapes. Une teinte par étape, jamais seule.

Pas de thème sombre dessiné (voir § 9).

### 2.2 Typographie

Polices Google Fonts : **Bitter** 600/700/800 (titres), **Karla** 400/500/700 (texte),
**IBM Plex Mono** 500 (données).

| Rôle | Style |
|---|---|
| Titre de voyage | Bitter 800, 33–36 px, interligne 1,08 |
| Titre d'écran | Bitter 800 (ou 700), 23–27 px |
| Titre de section | Bitter 800, 19–21 px |
| Titre d'élément | Bitter 700, 15–18 px |
| Texte | Karla 400/500, 13–15 px, interligne 1,45–1,55 |
| Boutons, liens | Karla 700, 13–14 px |
| Texte secondaire | Karla 12–13 px, `--texte2` |
| Données (dates, durées, étiquettes) | Plex Mono 500, 11–12 px, MAJUSCULES, interlettrage .02–.1em |
| Durée mise en avant | Plex Mono 19 px |

Exemples de données : `8 JANV → 29 JANV 2027 · 22 JOURS`, `5 NUITS`, `≈ 13 H 42 DE ROUTE`, `SAM. 9`.

### 2.3 Espacements, traits, coins, mouvement

- Espacements : 4 · 8 · 12 · 16 · 24 · 32 px. Marges du panneau : 24 px (22 px en haut du contenu).
- Traits : 1,5 px encre ; séparateurs 1,5 px pointillés `--pointille` ; 2 px + anneau 3 px moutarde
  pour un élément sélectionné (`box-shadow: 0 0 0 3px #E2B33C`, seul « box-shadow » autorisé).
- Coins : 2 px partout ; seuls les onglets ont 5 px en haut.
- Focus clavier : `outline: 3px solid #E2B33C; outline-offset: 2px`, toujours visible.
- Désactivé : 45 % d'opacité.
- Animations : 150–250 ms ; la carte « vole » vers un lieu en 400 ms. Tout est coupé sous
  `prefers-reduced-motion: reduce`.
- Barre de défilement du panneau : 8 px, pouce `#BDB39B`.
- Cibles tactiles : 44 px minimum sur mobile.

## 3. Composants

### Boutons

Base : Karla 700 14 px, `padding: 11px 18px`, trait 1,5 px encre, coins 2 px, fond transparent.
Un seul bouton principal par zone.

| Variante | Rendu | Exemple |
|---|---|---|
| Principal | fond forêt, texte papier ; survol `#244A40` | Ajouter au voyage |
| Secondaire | transparent, texte encre | Pas intéressé |
| Accent | fond moutarde | + Ajouter |
| Sombre | fond encre, texte papier ; confirmation d'une action lourde | Appliquer ce circuit |
| Danger | trait et texte alerte ; plein (fond alerte) dans la confirmation finale | Supprimer le voyage… |
| Petit | 12,5 px, `padding: 6px 12px` | Relancer |
| Emplacement vide | trait pointillé, texte forêt, pleine largeur | + Inviter quelqu'un |
| Lien | Karla 700, forêt, sans soulignement pour la navigation | ← Mes voyages |

### Champs et choix

- Champ : Karla 14 px, `padding: 10px 12px`, trait encre, fond `--carte`. Libellé au-dessus, 12 px `--texte2`, écart 4 px.
- Curseur (`range`) : `accent-color: --foret`, valeur affichée à droite du libellé en Plex Mono (`25 KM`, `3 H`).
- Sélecteur segmenté : boutons accolés dans un même cadre 1,5 px, séparés par des traits ; l'actif en encre plein, texte papier (Rythme : Tranquille / Équilibré / Soutenu ; Lieux célèbres / Régions / Circuits types).
- Puce de filtre : Karla 700 13 px, `padding: 6px 12px`, trait encre ; active = fond encre, texte papier.
- Grands choix (rythme dans Réglages) : bouton à deux lignes, titre + description.

### Onglets « chemise »

Onglets de dossier au-dessus du panneau : trait encre, coins 5 px en haut, fond `--onglet`,
Karla 700 12,5–13 px. L'actif prend le fond papier, perd son trait du bas et 2 px de hauteur en
plus : il se fond dans le panneau. Ordre : **Lieux · Idées · Itinéraire · Transports · Membres · Réglages**.

### Tampon de rôle

`ORGANISATEUR` : Plex Mono 11,5 px, trait 2 px forêt, texte forêt, pivoté de −4°. Dans Mes voyages,
−3° et gris `--texte2` pour un rôle autre qu'organisateur.

### Pastilles de statut

Plex Mono 11 px, `padding: 2px 7px`, trait 1,5 px.

| Pastille | Rendu |
|---|---|
| Planifié (`SAM. 9`) | fond moutarde |
| À planifier | trait pointillé `--texte2`, texte `--texte2` |
| En attente | fond surbrillance |
| Enregistré | fond forêt, texte papier |
| Fermé ce jour | trait et texte alerte |
| Neutre (`5 NUITS`) | trait encre seul |

### Éléments de liste

- **Ligne de lieu** : icône 34 px (carré forêt, pictogramme papier) · nom Bitter 700 15 px ·
  méta 12,5 px · pastille de statut à droite. Séparateur pointillé. Survol = fond surbrillance et
  repère entouré sur la carte.
- **Billet d'étape** : carte `--carte` ; colonne de 42 px « ÉTAPE » (Plex 10 px) + numéro Bitter 800
  28 px ; nom, pastille `N NUITS`, dates et durée en Plex Mono, points forts en texte.
- **Jour** : carte de 58 px, bandeau encre avec le jour en Plex moutarde (`SAM`), numéro Bitter 800 26 px.
- **Avatar** : carré encre 34–40 px, initiale Bitter 800 moutarde ; pastille de présence carrée
  11 px en coin (en ligne `--en-ligne`, absent `--pointille`).
- **Notice** : cadre encre, fond surbrillance, 13 px (« Long trajet : plus que les 3 h de route par jour visées. »).
- **Emplacement vide** : cadre pointillé, texte secondaire (« + Ajouter un restaurant »).

### Fenêtre modale

Voile `rgba(31,42,55,.62)`, boîte 760 px fond papier, trait 2 px encre. En-tête moutarde avec
titre Bitter 800 22 px et bouton fermer 34 px (`×`, `aria-label="Fermer"`). Actions alignées à droite :
Annuler (secondaire) puis action (sombre). `role="dialog" aria-modal="true"`.

### Toasts

En bas à gauche sur ordinateur, au-dessus des onglets sur mobile. Disparaissent après 6 s, sauf
l'erreur. Succès `✓` + action « Annuler » ; erreur `!` + « Réessayer » ; info `i`.

## 4. Mise en page

### 4.1 Coquille

- **Barre haute** 44 px (52 px sur mobile), fond encre : marque « TravelGuide » Bitter 800 18 px
  moutarde + slogan « Planifiez votre voyage, ensemble. » ; à droite statut serveur
  (carré 7 px vert, « Serveur opérationnel · v0.1.0 ») et « Se déconnecter ».
- **En-tête du voyage** (identique sur tous les onglets) : « ← Mes voyages », titre de voyage,
  tampon de rôle, dates + modes en Plex Mono, présence (« En direct · personne d'autre en ligne »).
  En mode étendu ou pleine largeur, il devient compact sur une ligne (titre 24 px, dates à droite).
  Dans une fiche, il se réduit à « ← Retour aux … » + nom du voyage.
- Puis les onglets chemise, puis le contenu défilant sur fond papier.

### 4.2 Trois largeurs de panneau

| Mode | Panneau | Carte | Écrans |
|---|---|---|---|
| 1 · Carte visible | 480 px | reste | Lieux, Idées, fiche d'un lieu, avis, zone d'attache, voyage vide |
| 2 · Panneau étendu | 820 px | ≈ 620 px | Itinéraire, Transports, Régions, Circuits types |
| 3 · Pleine largeur | contenu centré (max 1160 px ; 760 px pour Réglages) | masquée | Membres, Réglages, invitations |

- Mode 1 : survol d'une ligne → repère entouré ; clic → la carte zoome sur le lieu, le panneau
  montre sa fiche ; retour → vue d'ensemble.
- Mode 2 : choisir un jour ou un trajet cadre la carte dessus ; bouton « Plein écran carte » masque le panneau.
- Mode 3 : bouton « Voir la carte » (vignette de carte en coin) pour revenir.

### 4.3 Carte

- Style « carte ancienne » : mer `--mer`, terre `--terre` cernée d'un trait encre, parcs et reliefs
  hachurés `--relief` à 45°, routes `#B9AE8F`.
- Repères, distincts par la forme :
  - **Étape** : carré encre 34 px, numéro Bitter 800 moutarde, nom en Karla 700 avec halo terre.
  - **Lieu planifié / suggestion** : losange forêt, bord papier.
  - **Lieu à planifier** : losange papier, bord forêt.
  - **Sélection** : cercle encre autour du repère + étiquette encre (nom en papier) avec pointe.
  - **Zone d'attache** : cercle moutarde à 16 % d'opacité, trait moutarde pointillé `6 5` ;
    poignée de rayon ; le centre se déplace en glissant le carré.
  - **Trajet** : trait encre 5 px doublé d'un pointillé moutarde 2 px ; durées en étiquettes
    encre, texte moutarde Plex Mono (`3 H 25`).
  - **Badge de votes** : rectangle moutarde `2 / 3` (Plex Mono).
- Contrôles : bloc papier cerné en haut à droite, boutons 40 × 40 « + » / « − », « Tout voir ».
- Légende ou contexte en haut à gauche dans un cartouche papier (« Étape », « Suggestion » ;
  `TRAJET 1 · San José → Cahuita` ; `APERÇU · Variante 2 · non appliquée`).
- Mention en bas : « © contributeurs OpenStreetMap (ODbL) » et, quand c'est le cas,
  « TRAJETS ESTIMÉS À VOL D'OISEAU ».

## 5. Écrans (ordinateur, 1440 × 900)

### Lieux (mode 1)
Recherche « Rechercher un lieu, une adresse… » + bouton accent « + Ajouter ». Filtres :
Par zone · Par catégorie · À planifier · 5. Lieux **groupés par étape** : en-tête de groupe
(numéro, nom, `5 NUITS · 8 → 12 JANV`, `5 LIEUX`), lignes de lieu avec pastille Planifié ou À planifier.
Carte : panneau « AFFICHER » avec cases Étapes / Lieux planifiés / À planifier / Zones d'hébergement.

### Idées (mode 1)
« Suggestions » + explication du classement (goûts, notoriété Wikipédia / Wikivoyage / UNESCO,
temps libre ; OpenStreetMap, sans classement sponsorisé). Puces : À visiter · Nature · Sites · Musées ·
Activités · Où manger. Encadré moutarde « Circuit type · 22 nuits » (`SJO → SJO`), sélecteur de
variante, billets d'étape, total de route, bouton « Appliquer ce circuit ». Puis « Les plus réputés
du pays » : icône, nom, catégorie, raison (« Wikipédia en 19 langues · correspond à vos envies »),
Ajouter / Pas intéressé, liens Wikipédia · OSM. Pied : attribution + « Revoir les suggestions écartées ».

### Fiche d'un lieu (mode 1, carte zoomée)
Surtitre catégorie · parc, titre Bitter 800, distance (`≈ 40 MIN DE SAN JOSÉ`, `À VOL D'OISEAU`),
photo Wikimedia Commons créditée. « Pourquoi on vous le propose » (liste de raisons). Encadré
« Ajouter au voyage » : Zone d'hébergement (sélecteur), Jour (facultatif, « À définir plus tard »),
Note pour le groupe, Ajouter au voyage / Pas intéressé. Liens Wikipédia · Wikivoyage · OpenStreetMap.
« Autour de ce lieu » : nom, durée, « Voir ». Carte : bouton « Vue d'ensemble », cartouche de la zone.

### Avis sur un lieu (mode 1)
« Le groupe en pense quoi ? » — trois gros boutons de vote (72 px) **Envie · Pourquoi pas · Bof** ;
le choix courant en forêt avec anneau moutarde et « Votre choix », modifiable. Jauge empilée
(forêt / moutarde / vide), « 2 envies · 1 pourquoi pas · 0 bof », `3 / 3 ONT VOTÉ`, puis une ligne
par votant. « Commentaires » (avatar, nom, `HIER · 21 H 14`, texte) et champ « Ajouter un commentaire… » +
Publier. Pour l'organisateur : cadre pointillé « Retenir ce lieu ». Carte : badges `envies / total`.

### Itinéraire (mode 2)
Bandeau des 22 jours : boutons 32 px numérotés, liseré 5 px à la couleur de l'étape, jour choisi en
encre ; légende des étapes. Journée : « Samedi 9 janvier », ‹ ›, « Jour 2 sur 22 · nuit à San José »,
répartition `ROUTE 2 H 15 · VISITES 3 H · REPAS 1 H · LIBRE 2 H 30`. Frise horaire (`<ol>`) :
heure Plex Mono, point plein pour une visite (carte encadrée), rond creux pour départ / retour,
emplacements pointillés (« + Ajouter un restaurant (Où manger) », « Temps libre · suggestions à
proximité »). Pied : « Nuit à San José · 3 NUITS RESTANTES ». Colonne « À placer » (poignée ⋮⋮,
« + Jour »), aide au glisser-déposer, « Suggestions pour ce jour ». Carte cadrée sur le trajet du jour.

### Transports (mode 2)
« Trajets entre les étapes », `≈ 13 H 42 DE ROUTE · 6 TRAJETS`. Un billet par trajet (numéro,
`De → À`, date · mode, durée) ; le trajet ouvert est sélectionné (2 px + anneau moutarde) et montre :
Mode (Voiture · Bus · Avion · À pied), heure de départ, arrivée estimée et route, notice de long
trajet, puis « Où dormir ? » avec des étapes possibles sur la route et « Ajouter une étape ».

### Membres (mode 3)
« Membres 3 », « 1 personne en ligne ». Ligne par membre : avatar + présence, nom, statut, goûts
en puces, sélecteur RÔLE, menu « ⋯ ». « Invitations en attente » (Relancer / Annuler). Colonne de
droite : « Inviter quelqu'un » (e-mail, rôle, Envoyer l'invitation, ou lien à copier) ;
« Goûts du groupe » (jauge en 3 segments par goût, `3/3`) ; « Ce que permet chaque rôle »
(Organisateur, Éditeur, Lecteur).

### Réglages (mode 3)
Sommaire à gauche (Général · Rythme et déplacements · Suggestions · Notifications · Zone sensible ;
l'actif en moutarde). Sections : Général (nom, destination en lecture seule, début, fin) ; Rythme
(trois grands choix, temps de route maximum par jour 1–8 h, modes en puces, aéroports d'arrivée et
de départ) ; Suggestions (centres d'intérêt) ; Zone sensible (Quitter le voyage, Supprimer le voyage…).
Pied : « Enregistré automatiquement · il y a 2 s » — **pas de bouton Enregistrer**, changements visibles en direct.

### Zone d'attache (mode 1)
« NOUVELLE ZONE D'ATTACHE · Où dormez-vous ? ». Couverture des nuits (`5 / 22`, barre de 22 cases :
couvertes, cette zone en moutarde, vides en pointillé). Étapes numérotées : 1 Centre de la zone
(adresse, ville ou clic sur la carte), 2 Nuits (arrivée, départ, `5 NUITS`), 3 Rayon (5–80 km,
préréglages Autour de la ville · 10 / À la journée · 25 / Large · 50, Distance ou Temps de route).
Résultat en direct « 4 lieux dans la zone » + ce qu'apporterait un rayon plus large.
Créer la zone / Annuler. Carte : cercle réglable, légende Dans la zone / Hors zone.

### Voyage vide (mode 1)
« Votre voyage est encore vide » et trois portes d'entrée en grandes cartes-liens :
**Poser un point d'attache** (CONSEILLÉ), Partir d'une région, Appliquer un circuit type. Puis
« Ou partez des lieux les plus réputés » avec Ajouter. Carte : lieux célèbres en creux,
« Cliquez sur la carte pour poser un point d'attache ».

### Itinéraire sans hébergement
Jours non couverts : bordure pointillée, liseré pointillé alerte. Avertissement global
« 17 nuits sans hébergement · Du mer. 13 au ven. 29 janvier… » + « + Zone d'attache ». Jour vide :
« Rien de prévu ce jour-là », + Ajouter un lieu / Voir les suggestions, et « Jour de trajet probable … Voir les transports ».

### Mes voyages (pleine largeur, max 1240 px)
« BONJOUR ALEX · Mes voyages » + « + Nouveau voyage ». Invitation reçue (Accepter / Refuser).
Grille de 3 colonnes de cartes : vignette de carte, pays, nom, tampon de rôle, dates, jauge des
nuits couvertes (« 5 nuits sur 22 couvertes »), avatars. Dernière carte pointillée « Planifier un nouveau voyage ».

### Nouveau voyage
Panneau gauche « Où partez-vous ? » en trois étapes : 1 Destination (pays) + nom du voyage ;
2 Dates sur un calendrier mensuel (cases 40 px, bornes en encre/moutarde, plage en surbrillance,
`22 JOURS`) ; 3 Rythme et modes. Créer le voyage / Annuler, « Vous inviterez les autres voyageurs
ensuite, depuis l'onglet Membres. » Carte : aperçu de la destination.

### Idées › Régions (mode 2)
Sélecteur Lieux célèbres · Régions · Circuits types. Encadré « expédition » pour les lieux sans
route (Base Wafer, Isla del Coco : bateau ou avion). Une carte par région : nom, nuits conseillées,
catégories, points forts en liens, « Ajouter ces lieux » + « + Zone d'attache ». Survol = surbrillance
sur la liste et sur la carte.

### Idées › Circuits types (mode 2)
Aéroports d'arrivée / départ, « Proposer d'autres circuits ». Trois variantes en cartes-boutons
(nom, nombre d'étapes, barre des nuits par étape, résumé) ; choisie = surbrillance + anneau.
Détail de la variante (billets, durées, total de route). Barre d'action collée en bas :
explication + « Appliquer ce circuit… » (sombre). Carte en `APERÇU · non appliquée`.

### Confirmer un circuit (modale)
« Appliquer la Variante 2 ? ». Avant / après sur 22 cases de nuits. Liste des effets :
`+` zones créées (25 km, réglables), `+` points forts ajoutés, `≈` remplace les hébergements de ces
nuits, `=` conserve lieux, notes et membres. Notice : les autres membres le voient tout de suite,
« Annuler l'application » possible juste après. Annuler / Appliquer le circuit.

### Connexion et création de compte
Écran partagé : colonne formulaire 600 px fond papier (`padding: 40px 72px`, formulaire max 420 px),
carte décorative à droite avec slogan. Connexion : « Content de vous revoir », e-mail, mot de passe
(« Afficher », « Mot de passe oublié ? »), « Rester connecté sur cet appareil », Se connecter,
« Pas encore de compte ? Créer un compte », statut serveur + Confidentialité.
Création sur invitation : bandeau « Camille vous invite à rejoindre … comme éditeur », « Bienvenue
à bord », nom affiché, e-mail prérempli, mot de passe avec critères cochés en direct
(10 caractères ou plus), acceptation des conditions, « Créer mon compte et rejoindre ».
Colonne droite : « Ce qui vous attend » (3 points).

## 6. Mobile (390 × 844)

- Carte plein écran ; barre haute encre 52 px (marque, Menu, Tout voir).
- **Feuille** qui monte sur la carte : mi-hauteur (haut à 392 px) pour Lieux, pleine pour
  l'Itinéraire, avec « Voir la carte » pour redescendre.
- **Barre d'onglets** en bas, 64 px, fond encre, 5 entrées icône + libellé :
  Lieux · Idées · Itinéraire · Trajets · Membres (Réglages passe par le menu).
- Lignes de lieu de 64 px minimum ; bandeau de jours en 9 cases souples, 44 px de haut.
- Zone d'attache : feuille basse avec rayon, préréglages abrégés (Ville · 10 / Journée · 25 / Large · 50),
  résultat en direct, « Créer la zone » ; fermer `×` en haut.
- Connexion : bandeau de carte en haut, erreur globale `role="alert"` au-dessus du formulaire.

## 7. États d'erreur et cas limites

Règle commune : dire ce qui s'est passé, ce qui est conservé, et ce qu'on peut faire. Rouge alerte
réservé aux vrais problèmes, un avertissement reste moutarde. Bandeau d'erreur : trait alerte, fond
`--alerte-fond`, carré « ! » 22 px. Bandeau d'info : trait encre, fond surbrillance, carré « i ».

| # | Cas | Comportement |
|---|---|---|
| 1 | Connexion perdue | Bandeau sous la barre haute (« Hors connexion »), modifications gardées localement, Réessayer ; lecture possible, édition en direct suspendue ; « Reconnecté » 3 s au retour. |
| 2 | Conflit temps réel | « Camille a modifié ce jour en même temps » : les deux versions côte à côte, Garder la mienne / Prendre la sienne. Rien n'est écrasé en silence. |
| 3 | Trajet impossible | « Aucune route entre ces deux lieux » : Choisir bateau / Avion / Saisir la durée, plutôt qu'un blocage. |
| 4 | Champ invalide | Message sous le champ, bordure 2 px alerte, `aria-invalid`, bouton désactivé, focus sur le premier champ en erreur. |
| 5 | Suppression | Taper le nom du voyage pour activer « Supprimer définitivement » (danger plein) ; proposer « Quitter le voyage ». |
| 6 | Session expirée | Reconnexion sur place (mot de passe), brouillon et position de carte conservés. |
| 7 | Voyage introuvable | Même écran pour introuvable et accès refusé ; Mes voyages / Changer de compte. |
| 8 | Fond de carte indisponible | Repères, zones et tracés restent affichés sans tuiles ; Réessayer. |
| 9 | Suggestions indisponibles | Info moutarde, le reste fonctionne ; Réessayer / Ajouter un lieu à la main. |
| 10 | Chargement | Squelettes hachurés statiques (`#E6DFCC` / `#EFE9D8`), pas de clignotement ; texte alternatif « Chargement des lieux… ». |
| 11 | Aucun résultat | « Aucun lieu trouvé pour « … » », toujours une sortie : Effacer la recherche, élargir, ajouter à la main. |
| 12 | Toasts | Voir § 3. |

## 8. Accessibilité

- Contrastes : encre sur papier ≈ 13:1, `--texte2` sur papier ≈ 6:1. Ne pas mettre de texte blanc
  sur moutarde ni de texte `--pointille`.
- Vrais éléments : `<button>`, `<a href>`, `<label>` + champ ; `aria-label` sur les boutons
  icône (zoom, jour précédent / suivant, fermer, plus d'actions) ; onglets en `role="tab"` +
  `aria-selected`.
- Carte : `role="img"` + `aria-label` décrivant ce qui est montré ; la liste du panneau reste
  l'accès principal, la carte ne doit jamais être indispensable.
- Erreurs : `role="alert"` pour une erreur globale, `aria-invalid` pour un champ.

## 9. Écarts avec le code actuel et questions ouvertes

| Sujet | Aujourd'hui | Design | À décider |
|---|---|---|---|
| Palette | `--accent` teal `#0f766e`, fond blanc, coins 8/12/999 px | sable / papier / encre, coins 2 px | Remplacer les jetons dans `src/index.css`. |
| Thème sombre | `prefers-color-scheme` géré | non dessiné | Garder un sombre dérivé ou l'abandonner ? |
| Polices | system-ui | Bitter, Karla, IBM Plex Mono | Google Fonts ou auto-hébergées (confidentialité, hors ligne) ? |
| Icônes | emoji (🌐 📞 🗺️ 📍) | traits SVG, jamais d'emoji | Choisir un jeu d'icônes. |
| Onglets | « Suggestions », « Paramètres » (`i18n/fr.ts`) | « Idées », « Réglages » (« Trajets » sur mobile) | Renommer dans `fr.ts`. |
| Avis | note en étoiles (`Stars`, `StarInput`) | vote Envie / Pourquoi pas / Bof + commentaires + « Retenir ce lieu » | Changement de modèle côté backend (incrément 4). |
| Fond de carte | tuiles OpenFreeMap | style « carte ancienne » | Style MapLibre sur mesure à partir des tuiles OpenFreeMap. |
| Panneau | grille fixe `minmax(320px, 420px) 1fr` | 3 modes : 480 / 820 / pleine largeur | Mode porté par l'onglet actif. |
| Réglages | formulaire avec enregistrement | enregistrement automatique | Sauvegarde au fil de l'eau (débounce) via le temps réel. |
| Conflits, hors ligne | non gérés | § 7, cas 1, 2 et 6 | Dépend de l'incrément 8 (temps réel). |
| Notifications | absentes | section dans Réglages, non dessinée | À dessiner. |

Données d'exemple des maquettes (Costa Rica, Alex / Camille / Léa, « Variante 3 (exemple) »)
servent d'illustration ; aucun chiffre n'est à reprendre tel quel.
