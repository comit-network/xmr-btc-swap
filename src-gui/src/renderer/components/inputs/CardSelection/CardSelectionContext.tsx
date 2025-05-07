import { createContext, useContext, useState, ReactNode } from 'react'

interface CardSelectionContextType {
    selectedValue: string
    setSelectedValue: (value: string) => void
}

const CardSelectionContext = createContext<CardSelectionContextType | undefined>(undefined)

export function CardSelectionProvider({ 
    children, 
    initialValue,
    onChange 
}: { 
    children: ReactNode
    initialValue: string
    onChange?: (value: string) => void 
}) {
    const [selectedValue, setSelectedValue] = useState(initialValue)

    const handleValueChange = (value: string) => {
        setSelectedValue(value)
        onChange?.(value)
    }

    return (
        <CardSelectionContext.Provider value={{ selectedValue, setSelectedValue: handleValueChange }}>
            {children}
        </CardSelectionContext.Provider>
    )
}

export function useCardSelection() {
    const context = useContext(CardSelectionContext)
    if (context === undefined) {
        throw new Error('useCardSelection must be used within a CardSelectionProvider')
    }
    return context
} 