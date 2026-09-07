/*
 * ===========================================================================================
 * = COPYRIGHT
 *          PAX Computer Technology(Shenzhen) CO., LTD PROPRIETARY INFORMATION
 *   This software is supplied under the terms of a license agreement or nondisclosure
 *   agreement with PAX Computer Technology(Shenzhen) CO., LTD and may not be copied or
 *   disclosed except in accordance with the terms in that agreement.
 *     Copyright (C) 2022-? PAX Computer Technology(Shenzhen) CO., LTD All rights reserved.
 * Revision History:
 * Date	                Author	                Action
 * 20231010 	        JerryJia               	Create
 * ===========================================================================================
 */
package jp.gmofg.cps.https.msg;

public class ReverseData extends BaseData{

    private ReverseResult result;

    @Override
    public ReverseResult getBaseResult() {
        return result;
    }

    public void setResult(ReverseResult reverseResult) {
        this.result = reverseResult;
    }

    public static class ReverseResult extends BaseResult{

        private int amount;
        private String currencyCode;


        public int getAmount() {
            return amount;
        }
        public void setAmount(int amount) {
            this.amount = amount;
        }

        public String getCurrencyCode() {
            return currencyCode;
        }
        public void setCurrencyCode(String currencyCode) {
            this.currencyCode = currencyCode;
        }

    }
}
