-- Fix percentage values that are stored as 0-100 instead of 0-1
-- This migration converts percentage values for swap_ids where the sum > 1.0
-- by scaling all percentages for that swap_id by dividing by 100
UPDATE monero_addresses 
SET percentage = percentage / 100.0 
WHERE swap_id IN (
    SELECT swap_id 
    FROM monero_addresses 
    GROUP BY swap_id 
    HAVING SUM(percentage) > 1.0
); 