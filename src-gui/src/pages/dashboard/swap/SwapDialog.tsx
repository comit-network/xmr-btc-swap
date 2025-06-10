import Dialog from '@mui/material/Dialog'
import DialogTitle from '@mui/material/DialogTitle'
import { Typography } from '@mui/material'
import GetBitcoin from './getBitcoin/GetBitcoin'
import SelectMaker from './selectMaker/SelectMaker'
import { useState } from 'react'

type Step = 'getBitcoin' | 'selectMaker'

export default function SwapDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
    const [currentStep, setCurrentStep] = useState<Step>('getBitcoin')

    const handleNext = () => {
        if (currentStep === 'getBitcoin') {
            setCurrentStep('selectMaker')
        }
    }

    const handleBack = () => {
        if (currentStep === 'selectMaker') {
            setCurrentStep('getBitcoin')
        }
    }

    return (
        <Dialog open={open} onClose={onClose}>
            <DialogTitle>
                <Typography variant="h2">Swap</Typography>
            </DialogTitle>
            {currentStep === 'getBitcoin' && <GetBitcoin onNext={handleNext} />}
            {currentStep === 'selectMaker' && <SelectMaker onNext={() => {}} onBack={handleBack} />}
        </Dialog>
    )
}
