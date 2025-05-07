import { Box } from "@material-ui/core";
import CheckIcon from '@material-ui/icons/Check'
import { useCardSelection } from './CardSelectionContext'

// The value prop is used by the parent CardSelectionGroup to determine which option is selected
export default function CardSelectionOption({children, value}: {children: React.ReactNode, value: string}) {
    const { selectedValue, setSelectedValue } = useCardSelection()
    const selected = value === selectedValue

    return (
        <Box
            onClick={() => setSelectedValue(value)}
            style={{
                display: 'flex',
                alignItems: 'flex-start',
                gap: 16,
                border: selected ? '2px solid #FF5C1B' : '2px solid #555',
                borderRadius: 16,
                padding: '1em',
                cursor: 'pointer',
                transition: 'all 0.2s ease-in-out',
            }}
        >
            <Box
                style={{
                    border: selected ? '2px solid #FF5C1B' : '2px solid #555',
                    borderRadius: 99999,
                    width: 28,
                    height: 28,
                    background: selected ? '#FF5C1B' : 'transparent',
                    overflow: 'hidden',
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                    transition: 'all 0.2s ease-in-out',
                    transform: selected ? 'scale(1.1)' : 'scale(1)',
                    flexShrink: 0,
                }}
            >
                {selected ? (
                    <CheckIcon 
                        style={{ 
                            transition: 'all 0.2s ease-in-out',
                            transform: 'scale(1)',
                            animation: 'checkIn 0.2s ease-in-out'
                        }} 
                    />
                ) : null}
            </Box>
            <Box pt={0.5}>{children}</Box>
        </Box>
    )
}