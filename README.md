# Running the project
1. Ensure you have rust installed
2. Run the following commands
```bash
git clone https://github.com/espieux/user_op_indexer.git
cd user_op_indexer
cargo build
cargo run
```

# Workshop: Indexing UserOperationEvent from EntryPoint Contract
**Duration**: 3 heures
**Objectif**: Développer un indexeur pour les événements UserOperationEvent de l'EntryPoint ERC-4337 (address: 0x0000000071727de22e5e9d8baf0edac6f37da032)  (topic: 0x49628fd1471006c1482da88028e9ce4dbb080b815c9b0344d39e5a8e6ec1419f)

## Partie 1: Setup 
1. Créez un nouveau projet Node.js ou rust ✅
2. Installez les dépendances nécessaires ✅

## Partie 2: Connexion à l'Ethereum
1. Créez une connexion à un nœud Ethereum (Infura, Alchemy, etc.) ✅
2. Définissez l'interface de l'événement UserOperationEvent avec les champs suivants: ✅
   ```typescript
   userOpHash: string     // bytes32
   sender: string        // address
   paymaster: string     // address
   nonce: bigint        // uint256
   success: boolean     // bool
   actualGasCost: bigint // uint256
   actualGasUsed: bigint // uint256
   ```

## Partie 3: Implémentation de l'Indexeur 
Implémentez la logique pour:
   - Écouter les nouveaux événements en temps réel ✅
   - Récupérer les événements historiques si un bloc de départ est spécifié ✅
   - Gérer les reconnexions en cas d'erreur ✅
   - Sauvegarder les événements dans la base de données ✅

## Partie 4: Persistance et Requêtes (45 minutes)
1. Créez un schéma de base de données approprié ✅
2. Implémentez dès fonctions pour:
   - Sauvegarder les nouveaux événements ✅
   - Requêter les événements par:
     - userOpHash ✅
     - sender ✅
     - paymaster ✅
     - plage de blocs ✅
     - statut (success) ✅

User interface
- Ajoutez une interface API REST ou GraphQL
- Implémentez un mécanisme de cache
- Ajoutez des métriques et du monitoring
- Créez une interface utilisateur simple (frontend) pour visualiser les userOperations en temps réel

## Critères d'Évaluation
- Qualité et organisation du code
- Gestion des erreurs
- Performance de l'indexation
- Complétude des fonctionnalités
- Documentation
- Tests

## Livrables
1. Code source sur GitHub
2. Documentation d'installation et d'utilisation
3. Un rapport bref expliquant:
   - L'architecture choisie
   - Les défis rencontrés
   - Les optimisations possibles

## Notes Importantes
- Implémentez un mécanisme de reprise après arrêt
- Pour tester votre script, vous pouvez créer un contrat qui émet un userOperation event a votre demande.   
- Gérez correctement les types (u128 / u256 notamment)
- Documentez vos choix techniques

