// Toutes les chaînes de l'interface sont regroupées ici pour faciliter l'i18n plus tard.
export const fr = {
  appName: 'TravelGuide',
  tagline: 'Planifiez votre voyage, ensemble.',
  health: {
    checking: 'Vérification du serveur…',
    ok: 'Serveur opérationnel',
    down: 'Serveur injoignable',
  },
  common: {
    loading: 'Chargement…',
    email: 'Adresse email',
    password: 'Mot de passe',
    displayName: 'Prénom ou pseudo',
    save: 'Enregistrer',
    unexpectedError: 'Une erreur est survenue. Réessayez.',
  },
  nav: {
    account: 'Mon compte',
    logout: 'Se déconnecter',
    home: 'Accueil',
  },
  login: {
    title: 'Connexion',
    withPassword: 'Avec mot de passe',
    withMagicLink: 'Par lien magique',
    submit: 'Se connecter',
    sendLink: 'Recevoir un lien de connexion',
    linkSent:
      'Si un compte existe pour cette adresse, un lien de connexion vient de vous être envoyé. Il expire dans 15 minutes.',
    invalidCredentials: 'Email ou mot de passe incorrect.',
    tooManyAttempts: 'Trop de tentatives. Patientez quelques minutes.',
    noAccount: 'Pas encore de compte ?',
    register: 'Créer un compte',
  },
  register: {
    title: 'Créer un compte',
    submit: 'Créer mon compte',
    passwordHint: 'Au moins 10 caractères.',
    haveAccount: 'Déjà un compte ?',
    login: 'Se connecter',
  },
  magic: {
    verifying: 'Connexion en cours…',
    invalid: 'Ce lien est invalide ou a expiré.',
    retry: 'Demander un nouveau lien',
  },
  account: {
    title: 'Mon compte',
    profile: 'Profil',
    plan: 'Abonnement',
    plans: { free: 'Gratuit', organizer: 'Organisateur' } as Record<string, string>,
    saved: 'Modifications enregistrées.',
    passwordSection: 'Mot de passe',
    setPassword: 'Définir un mot de passe',
    changePassword: 'Changer le mot de passe',
    currentPassword: 'Mot de passe actuel',
    newPassword: 'Nouveau mot de passe',
    passwordChanged: 'Mot de passe mis à jour. Vos autres sessions ont été fermées.',
    noPasswordYet: 'Votre compte n’a pas encore de mot de passe : vous vous connectez par lien magique.',
    dataSection: 'Mes données (RGPD)',
    dataIntro:
      'Vos données ne sont ni revendues ni partagées. Vous pouvez les exporter ou supprimer votre compte à tout moment.',
    export: 'Exporter mes données (JSON)',
    deleteTitle: 'Supprimer mon compte',
    deleteWarning:
      'Action définitive. Vos avis seront supprimés ; les voyages que vous organisez seront transférés au plus ancien éditeur, ou supprimés s’il n’y en a pas.',
    confirmEmail: 'Tapez votre email pour confirmer',
    deleteSubmit: 'Supprimer définitivement',
  },
} as const

export const t = fr
