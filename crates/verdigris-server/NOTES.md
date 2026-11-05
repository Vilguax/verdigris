# Notes du serveur Verdigris

## Commande /give

- Verdigris n'a pas d'entité objet au sol ni de son de ramassage. Le surplus d'un
  don qui ne tient pas dans les 36 cases de l'inventaire est donc perdu
  silencieusement, contrairement au serveur vanilla qui ferait tomber l'objet au sol.
- Le plafond de refus est `max_stack_size x 100` (par exemple 1600 pour une perle de
  l'Ender dont la pile vaut 16). Au-delà, aucun inventaire n'est modifié et aucun
  paquet d'inventaire n'est émis.
- Les tailles de pile proviennent uniquement de `item_props.<nom>.max_stack_size`
  dans `blocks.json`, préchargées une fois. Aucune valeur n'est codée en dur.

## Points INCERTAIN du composant objet

- La couleur de rareté est toujours `white` faute de données de rareté dans
  `blocks.json`. Un objet épique ou légendaire devrait afficher une autre couleur.
- Les clés de traduction des objets spéciaux (bannières, potions à effets, têtes)
  ne sont pas différenciées : seule la distinction bloc (`block.minecraft.*`) vs
  objet (`item.minecraft.*`) est gérée.

## Numérotation de l'arbre de commandes

- Le noeud `give` et ses trois arguments sont ajoutés en fin de liste de noeuds
  (append-only), ce qui diverge de la renumérotation BFS stricte de vanilla mais
  préserve l'invariance octet pour octet des sous-arbres des commandes existantes.
  La conformité réelle côté client est validée par le scénario d'intégration TCP
  Alice/Bob qui exécute `/give` avec un flux client complet.
