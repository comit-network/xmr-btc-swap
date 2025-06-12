import { Box, Typography } from '@mui/material'
import Avatar from 'boring-avatars'
import { AccessTimeOutlined as ClockIcon } from '@mui/icons-material'
import { MonetizationOnOutlined as MoneyIcon } from '@mui/icons-material'
import { CurrencyBitcoinOutlined as BitcoinIcon } from '@mui/icons-material'
import IconChip from '../IconChip'

export default function MakerOfferItem() {
    return (
        <Box
            sx={{
                display: 'flex',
                flexDirection: 'row',
                gap: 2,
                border: '1px solid',
                borderColor: 'divider',
                borderRadius: 2,
                padding: 2,
            }}
        >
            <Avatar
                size={40}
                name="Maria Mitchell"
                variant="marble"
                colors={['#92A1C6', '#146A7C', '#F0AB3D', '#C271B4', '#C20D90']}
            />
            <Box
                sx={{
                    display: 'flex',
                    flexDirection: 'column',
                    gap: 1,
                }}
            >
                <Typography variant="h4">Maria Mitchell</Typography>
                <Typography variant="body1">fjklsdfjlfdk</Typography>
                <Box
                    sx={{
                        display: 'flex',
                        flexDirection: 'row',
                        gap: 1,
                    }}
                >
                    <IconChip icon={<ClockIcon />} color="primary.main">
                        active for <Typography sx={{
                            fontWeight: 800,
                            fontSize: 12,
                        }}>10 minutes</Typography>
                    </IconChip>
                    <IconChip icon={<MoneyIcon />} color="primary.main">
                        <Typography sx={{
                            fontWeight: 800,
                            fontSize: 12,
                        }}>0.12 %</Typography> fee
                    </IconChip>
                    <IconChip icon={<BitcoinIcon />} color="primary.main">
                        <Typography sx={{
                            fontWeight: 800,
                            fontSize: 12,
                        }}>0.00003 – 0.00500</Typography>
                    </IconChip>
                </Box>
            </Box>
        </Box>
    )
}
