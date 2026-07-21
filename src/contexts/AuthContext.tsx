import React, { createContext } from "react";

export interface LocalUser {
  id: string;
  email: string;
}

export interface AuthContextType {
  user: LocalUser;
  loading: boolean;
  signIn: (_email: string, _password: string) => Promise<void>;
  signUp: (_email: string, _password: string) => Promise<void>;
  signOut: () => Promise<void>;
}

const LOCAL_USER: LocalUser = {
  id: "local-user",
  email: "local@vectorflow",
};

const noopAsync = async () => {};

export const AuthContext = createContext<AuthContextType | undefined>(undefined);

interface AuthProviderProps {
  children: React.ReactNode;
}

export const AuthProvider: React.FC<AuthProviderProps> = ({ children }) => {
  const value: AuthContextType = {
    user: LOCAL_USER,
    loading: false,
    signIn: noopAsync,
    signUp: noopAsync,
    signOut: noopAsync,
  };

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
};
